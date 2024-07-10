#![deny(warnings)]
use renderer::Renderer;
use std::{io::Result, sync::Arc};
use vulkanalia::vk::DeviceV1_0 as _;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

pub mod renderer;

struct ServerState {
	clients: Vec<wayland_server::Client>,
	window: Option<Window>,
	renderer: Option<Renderer>,
}

impl ApplicationHandler for ServerState {
	fn resumed(&mut self, event_loop: &ActiveEventLoop) {
		unsafe {
			self.window = Some(
				event_loop
					.create_window(Window::default_attributes())
					.unwrap_or_else(|err| {
						tracing::error!("Failed to create window: {}", err);
						std::process::exit(1);
					}),
			);

			let window = self.window.as_ref().unwrap();

			self.renderer = Some(Renderer::new(window).unwrap_or_else(|err| {
				tracing::error!("Failed to create renderer: {}", err);
				std::process::exit(1);
			}));
		}
	}

	fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
		unsafe {
			self.renderer
				.as_mut()
				.unwrap()
				.device
				.device_wait_idle()
				.unwrap();
		}
	}

	fn window_event(
		&mut self,
		event_loop: &ActiveEventLoop,
		_id: WindowId,
		event: WindowEvent,
	) {
		match event {
			WindowEvent::CloseRequested => {
				event_loop.exit();
			}
			WindowEvent::RedrawRequested => {
				unsafe {
					// Draw.
					self.renderer
						.as_mut()
						.unwrap()
						.render_frame()
						.unwrap_or_else(|err| {
							tracing::error!(
								"Failed to create renderer: {}",
								err
							);
							std::process::exit(1);
						});
				}

				self.window.as_ref().unwrap().request_redraw();
			}
			_ => (),
		}
	}
}

#[derive(Debug, Default)]
struct ClientState;

impl wayland_server::backend::ClientData for ClientState {
	fn initialized(&self, client_id: wayland_server::backend::ClientId) {
		tracing::info!("Client initialized: {:?}", client_id);
	}
	fn disconnected(
		&self,
		client_id: wayland_server::backend::ClientId,
		disconnect_reason: wayland_server::backend::DisconnectReason,
	) {
		tracing::info!(
			"Client {:?} disconnected: {:?}",
			client_id,
			disconnect_reason
		);
	}
}

fn main() -> Result<()> {
	tracing_subscriber::fmt::init();

	let socket =
		wayland_server::ListeningSocket::bind_auto("wayland", 0..).unwrap();

	tracing::info!("Listening on socket: {:?}", socket.socket_name().unwrap());

	let event_loop = EventLoop::new().unwrap();
	event_loop.set_control_flow(ControlFlow::Poll);
	event_loop.set_control_flow(ControlFlow::Wait);

	let mut state = ServerState {
		clients: Vec::new(),
		window: None,
		renderer: None,
	};

	let mut display = wayland_server::Display::new().unwrap_or_else(|err| {
		tracing::error!("Failed to create display: {}", err);
		std::process::exit(1);
	});

	event_loop.run_app(&mut state).unwrap_or_else(|err| {
		tracing::error!("Failed to run application: {}", err);
		std::process::exit(1);
	});

	loop {
		if let Some(stream) = socket.accept().unwrap() {
			let client = display
				.handle()
				.insert_client(stream, Arc::new(ClientState))
				.unwrap();
			state.clients.push(client);
		}

		display.dispatch_clients(&mut state)?;
		display.flush_clients()?;
	}
}
