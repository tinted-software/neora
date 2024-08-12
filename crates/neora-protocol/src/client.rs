use crate::{
	Event, WaylandSocket, WlCallback, WlCallbackEvent, WlDisplay, WlObject,
	WlRawObject,
};
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::{Arc, Condvar, RwLock};

pub trait EventHandler: Send + Sync {
	fn handle(&mut self, event: &Event);
}

pub struct Client<Socket: WaylandSocket> {
	pub socket: Socket,
	pub obj_map: Arc<Mutex<HashMap<u32, Arc<WlObject>>>>,
	pub max_id: Arc<Mutex<u32>>,
	pub event_listeners: Arc<RwLock<Vec<Box<dyn EventHandler>>>>,
}

impl<Socket: WaylandSocket> Drop for Client<Socket> {
	fn drop(&mut self) {
		self.disconnect()
	}
}

impl<Socket: WaylandSocket> Client<Socket> {
	pub fn connect(socket: Socket) -> Client<Socket> {
		let client = Client {
			socket,
			obj_map: Arc::new(Mutex::new(HashMap::new())),
			max_id: Arc::new(Mutex::new(0)),
			event_listeners: Arc::new(RwLock::new(Vec::new())),
		};
		client.bind_object::<WlDisplay>(1);

		return client;
	}

	pub fn start_event_loop(&mut self) {
		let evs = self.socket.read_event();
		for (raw_event_header, msg_body) in evs {
			let sender = self.get_object(raw_event_header.sender_id).unwrap();
			let event = sender.parse_event(
				&mut self.socket,
				raw_event_header.sender_id,
				raw_event_header.op_code,
				msg_body,
			);
			tracing::info!("{:?}", event);

			for event_handler in
				self.event_listeners.write().unwrap().iter_mut()
			{
				event_handler.handle(&event);
			}
		}
	}

	pub fn get_display(&self) -> WlDisplay {
		self.get_object(1).unwrap().try_get_wl_display().unwrap()
	}

	pub fn sync(&mut self) {
		let callback_id = self.new_object::<WlCallback>();

		let done_pair = Arc::new((Mutex::new(false), Condvar::new()));
		let c_done_pair = done_pair.clone();
		self.add_event_listener(SyncListener {
			callback_id,
			c_done_pair,
		});
		self.get_display().sync(&mut self.socket, callback_id);

		let &(ref done, ref cond_var) = &*done_pair;
		let mut done = done.lock().unwrap();
		while !*done {
			done = cond_var.wait(done).unwrap();
		}
	}

	pub fn get_object(&self, obj_id: u32) -> Option<Arc<WlObject>> {
		Some(self.obj_map.lock().unwrap().get(&obj_id)?.clone())
	}

	pub fn new_object<T: WlRawObject>(&self) -> u32 {
		let mut hash_map = self.obj_map.lock().unwrap();
		let mut now_max = self.max_id.lock().unwrap();

		let new_id = now_max.clone() + 1;
		let wl_obj = Arc::new(T::new(new_id).to_enum());
		hash_map.insert(new_id, wl_obj.clone());
		*(now_max) += 1;

		return new_id;
	}

	pub fn delete_obj(&self, obj_id: u32) {
		self.obj_map.lock().unwrap().remove(&obj_id);
	}

	pub fn bind_object<T: WlRawObject>(&self, obj_id: u32) {
		let wl_obj = Arc::new(T::new(obj_id).to_enum());
		self.obj_map.lock().unwrap().insert(obj_id, wl_obj.clone());

		let now_max = self.max_id.lock().unwrap().clone();
		*(self.max_id.lock().unwrap()) = std::cmp::max(now_max, obj_id);
	}

	pub fn add_event_listener(
		&mut self,
		event_handler: impl EventHandler + 'static,
	) {
		self.event_listeners
			.write()
			.unwrap()
			.push(Box::new(event_handler));
	}

	pub fn disconnect(&mut self) {
		self.socket.disconnect();
	}
}

struct SyncListener {
	callback_id: u32,
	c_done_pair: Arc<(Mutex<bool>, Condvar)>,
}

impl EventHandler for SyncListener {
	fn handle(&mut self, event: &Event) {
		match event {
			Event::WlCallbackEvent(callback_ev) => match callback_ev {
				WlCallbackEvent::WlCallbackdoneEvent(done)
					if done.sender_id == self.callback_id =>
				{
					let &(ref done, ref cond_var) = &*self.c_done_pair;
					*(done.lock().unwrap()) = true;
					cond_var.notify_all();
				}
				_ => {}
			},
			_ => {}
		}
	}
}
