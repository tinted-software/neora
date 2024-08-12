use std::{
	num::NonZero,
	os::fd::BorrowedFd,
	sync::{Arc, Mutex},
};

use neora_protocol::{
	client::{Client, EventHandler},
	unix::WaylandUnixSocket,
	Event, WaylandSocket, WlBuffer, WlCallback, WlCallbackEvent, WlCompositor,
	WlDisplayEvent, WlRegistry, WlRegistryEvent, WlShell, WlShellSurface,
	WlShellSurfaceEvent, WlShm, WlShmPool, WlSurface,
};
use nix::fcntl::{fcntl, FcntlArg, FdFlag};
use tracing::info;
use tracing_error::ErrorSubscriber;
use tracing_subscriber::{subscribe::CollectExt, util::SubscriberInitExt};

#[tracing::instrument]
fn main() {
	tracing_subscriber::registry()
		.with(tracing_subscriber::fmt::subscriber())
		// The `ErrorSubscriber` subscriber layer enables the use of `SpanTrace`.
		.with(ErrorSubscriber::default())
		.init();

	let socket = WaylandUnixSocket::connect(None);
	let mut client = Client::<WaylandUnixSocket>::connect(socket);

	std::thread::spawn(move || {
		client.start_event_loop();
	});

	info!("Connected to display");

	let wl_compositor_id = Arc::new(Mutex::new(0));
	let c_wl_compositor_id = wl_compositor_id.clone();

	let wl_shell_id = Arc::new(Mutex::new(0));
	let c_wl_shell_id = wl_shell_id.clone();

	let wl_shm_id = Arc::new(Mutex::new(0));
	let c_wl_shm_id = wl_shm_id.clone();

	client.add_event_listener(EventListener1 {
		client: &mut client,
		c_wl_compositor_id,
		c_wl_shell_id,
		c_wl_shm_id,
	});

	client.bind_object::<WlRegistry>(2);
	client.get_display().get_registry(&mut client.socket, 2);
	info!("Get Registry at id 2");
	client.sync();
	info!("Wayland Sync");

	let wl_compositor_id = *(wl_compositor_id.lock().unwrap());
	let wl_shell_id = *(wl_shell_id.lock().unwrap());
	let wl_shm_id = *(wl_shm_id.lock().unwrap());

	let wl_surface_id = client.new_object::<WlSurface>();
	client
		.get_object(wl_compositor_id)
		.unwrap()
		.try_get_wl_compositor()
		.unwrap()
		.create_surface(&mut client.socket, wl_surface_id);
	let wl_surface = client
		.get_object(wl_surface_id)
		.unwrap()
		.try_get_wl_surface()
		.unwrap();

	let wl_shell_surface_id = client.new_object::<WlShellSurface>();
	client.add_event_listener(ShellSurfaceHandler {
		client: &mut client,
	});
	let wl_shell = client
		.get_object(wl_shell_id)
		.unwrap()
		.try_get_wl_shell()
		.unwrap();
	wl_shell.get_shell_surface(
		&mut client.socket,
		wl_shell_surface_id,
		wl_surface_id,
	);

	let wl_shell_surface = client
		.get_object(wl_shell_surface_id)
		.unwrap()
		.try_get_wl_shell_surface()
		.unwrap();
	wl_shell_surface.set_toplevel(&mut client.socket);

	let width = 480;
	let height = 360;
	let size = width * height * 4;

	let (buffer_fd, buffer_file_name) = nix::unistd::mkstemp(
		&std::path::Path::new(&std::env::var("XDG_RUNTIME_DIR").unwrap())
			.join("weston-shared-XXXXXX"),
	)
	.unwrap();

	let buffer_fd_flags =
		FdFlag::from_bits(fcntl(buffer_fd, FcntlArg::F_GETFD).unwrap())
			.unwrap();
	fcntl(
		buffer_fd,
		nix::fcntl::F_SETFD(FdFlag::FD_CLOEXEC | buffer_fd_flags),
	)
	.unwrap();

	nix::unistd::unlink(&buffer_file_name).unwrap();
	nix::unistd::ftruncate(unsafe { BorrowedFd::borrow_raw(buffer_fd) }, size)
		.unwrap();
	let shm_data = unsafe {
		std::slice::from_raw_parts_mut(
			nix::sys::mman::mmap(
				None,
				NonZero::new(size as usize).unwrap(),
				nix::sys::mman::ProtFlags::PROT_READ
					| nix::sys::mman::ProtFlags::PROT_WRITE,
				nix::sys::mman::MapFlags::MAP_SHARED,
				BorrowedFd::borrow_raw(buffer_fd),
				0,
			)
			.unwrap()
			.as_mut() as *mut _ as *mut u32,
			(width * height) as usize,
		)
	};
	for i in 0..(width * height) as usize {
		shm_data[i] = 0xffff;
	}

	let dup_fd = fcntl(buffer_fd, nix::fcntl::F_DUPFD_CLOEXEC(0)).unwrap()
		as std::os::unix::io::RawFd;
	let wl_shm = client
		.get_object(wl_shm_id)
		.unwrap()
		.try_get_wl_shm()
		.unwrap();
	let wl_shm_pool_id = client.new_object::<WlShmPool>();
	wl_shm.create_pool(&mut client.socket, wl_shm_pool_id, dup_fd, size as i32);
	let wl_shm_pool = client
		.get_object(wl_shm_pool_id)
		.unwrap()
		.try_get_wl_shm_pool()
		.unwrap();
	nix::unistd::close(dup_fd).unwrap();

	let wl_buffer = client.new_object::<WlBuffer>();
	wl_shm_pool.create_buffer(
		&mut client.socket,
		wl_buffer,
		0,
		width as i32,
		height as i32,
		(width * 4) as i32,
		1,
	);

	wl_surface.attach(&mut client.socket, wl_buffer, 0, 0);
	wl_surface.commit(&mut client.socket);

	let wl_callback = client.new_object::<WlCallback>();
	let c_wl_callback = wl_callback.clone();

	client.add_event_listener(DrawEventHandler { c_wl_callback });
	wl_surface.frame(&mut client.socket, wl_callback);

	client.sync();
	loop {}
}

struct DrawEventHandler {
	c_wl_callback: u32,
}
impl EventHandler for DrawEventHandler {
	fn handle(&mut self, event: &Event) {
		match event {
			Event::WlCallbackEvent(callback_ev) => match callback_ev {
				WlCallbackEvent::WlCallbackdoneEvent(done) => {
					if done.sender_id == self.c_wl_callback {
						info!("Redraw");
					}
				}
			},
			_ => {}
		}
	}
}

struct EventListener1<'a, Socket: WaylandSocket> {
	client: &'a mut Client<Socket>,
	c_wl_compositor_id: Arc<Mutex<u32>>,
	c_wl_shell_id: Arc<Mutex<u32>>,
	c_wl_shm_id: Arc<Mutex<u32>>,
}

impl<Socket: WaylandSocket> EventHandler for EventListener1<'_, Socket> {
	fn handle(&mut self, event: &Event) {
		match event {
			Event::WlDisplayEvent(display_ev) => match display_ev {
				WlDisplayEvent::WlDisplaydeleteIdEvent(rm_id_ev) => {
					self.client.delete_obj(rm_id_ev.id);
				}
				_ => {}
			},
			Event::WlRegistryEvent(reg_ev) => match reg_ev {
				WlRegistryEvent::WlRegistryglobalEvent(gl_ev) => {
					let wl_registry = self
						.client
						.get_object(gl_ev.sender_id)
						.unwrap()
						.try_get_wl_registry()
						.unwrap();

					info!(
						"WlRegistryGlobalEvent: Name: {}, Interface: {}",
						gl_ev.name, gl_ev.interface
					);
					if gl_ev.interface == "wl_compositor" {
						let mut obj_id =
							self.c_wl_compositor_id.lock().unwrap();
						*obj_id = self.client.new_object::<WlCompositor>();
						wl_registry.bind(
							&mut self.client.socket,
							gl_ev.name,
							*obj_id,
						);
					} else if gl_ev.interface == "wl_shell" {
						let mut obj_id = self.c_wl_shell_id.lock().unwrap();
						*obj_id = self.client.new_object::<WlShell>();
						wl_registry.bind(
							&mut self.client.socket,
							gl_ev.name,
							*obj_id,
						);
					} else if gl_ev.interface == "wl_shm" {
						let mut obj_id = self.c_wl_shm_id.lock().unwrap();
						*obj_id = self.client.new_object::<WlShm>();
						wl_registry.bind(
							&mut self.client.socket,
							gl_ev.name,
							*obj_id,
						);
					}
				}
				_ => {}
			},
			_ => {}
		}
	}
}

struct ShellSurfaceHandler<'a, Socket: WaylandSocket> {
	client: &'a mut Client<Socket>,
}

impl<Socket: WaylandSocket> EventHandler for ShellSurfaceHandler<'_, Socket> {
	fn handle(&mut self, event: &Event) {
		match event {
			Event::WlShellSurfaceEvent(wl_shell_surface_ev) => {
				match wl_shell_surface_ev {
					WlShellSurfaceEvent::WlShellSurfacepingEvent(ping_ev) => {
						self.client
							.get_object(ping_ev.sender_id)
							.unwrap()
							.try_get_wl_shell_surface()
							.unwrap()
							.pong(&mut self.client.socket, ping_ev.serial);
					}
					_ => {}
				}
			}
			_ => {}
		}
	}
}
