#![feature(stmt_expr_attributes)]
#![allow(unused_variables)]

use tracing::{info, warn};

pub trait ReadEvent {
	fn read_event(&mut self) -> Vec<(EventHeader, Vec<u8>)>;
}

pub trait WaylandSocket: ReadEvent + Send + Sync {
	fn connect(name: Option<&str>) -> Self
	where
		Self: Sized;
	fn send(&mut self, buffer: &[u8], fd: &[i32]);
	fn disconnect(&mut self);
}

include!(concat!(env!("OUT_DIR"), "/wayland.rs"));

#[cfg(target_family = "unix")]
pub mod unix;

pub mod client;
