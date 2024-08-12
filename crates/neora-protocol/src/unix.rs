use crate::{EventHeader, WaylandSocket};
use crate::{EventHeaderPre, ReadEvent};
use byteorder::WriteBytesExt;
use nix::cmsg_space;
use nix::sys::socket::MsgFlags;
use nix::sys::socket::{self, UnixAddr};
use nix::sys::socket::{recvmsg, sendmsg};
use nix::sys::socket::{ControlMessage, ControlMessageOwned};
use std::io::Cursor;
use std::io::IoSlice;
use std::io::IoSliceMut;
use std::mem::transmute;
use std::os::fd::{AsRawFd, OwnedFd};
use std::os::unix::io::RawFd;
use tracing::warn;

pub struct UnixSocket {
	fd: OwnedFd,
}

impl UnixSocket {
	pub fn connect(path: std::path::PathBuf) -> UnixSocket {
		let fd = socket::socket(
			socket::AddressFamily::Unix,
			socket::SockType::Stream,
			socket::SockFlag::SOCK_CLOEXEC,
			None,
		)
		.unwrap();

		socket::connect(fd.as_raw_fd(), &UnixAddr::new(&path).unwrap())
			.unwrap();

		return UnixSocket { fd };
	}

	pub fn write(&mut self, buffer: &[u8], fds: &[RawFd]) {
		let iov = [IoSlice::new(buffer); 1];
		let cmsg = [ControlMessage::ScmRights(fds)];
		sendmsg::<()>(
			self.fd.as_raw_fd(),
			&iov,
			&cmsg,
			MsgFlags::empty(),
			None,
		)
		.unwrap();
	}

	#[tracing::instrument(skip(self))]
	pub fn read(&mut self, buffer: &mut [u8], fds: &mut [u8]) -> (usize, i32) {
		let mut iov = [IoSliceMut::new(buffer); 1];
		let mut cmsg = cmsg_space!([RawFd; 1]);

		let msg = recvmsg::<()>(
			self.fd.as_raw_fd(),
			&mut iov,
			Some(&mut cmsg),
			MsgFlags::empty(),
		)
		.unwrap();

		let mut num_fds = 0;
		let mut buf = Cursor::new(fds);
		for cmsg in msg.cmsgs().unwrap() {
			match cmsg {
				ControlMessageOwned::ScmRights(newfds) => {
					buf.write_i32::<byteorder::NativeEndian>(newfds[0])
						.unwrap();
					num_fds += 1;
				}
				_ => {}
			}
		}

		return (msg.bytes, num_fds);
	}

	pub fn shutdown(&mut self) {
		unimplemented!()
	}
}

pub struct WaylandUnixSocket {
	socket: UnixSocket,
}

impl WaylandUnixSocket {
	pub fn connect(name: Option<&str>) -> WaylandUnixSocket {
		let default_name =
			std::env::var("WAYLAND_DISPLAY").unwrap_or("wayland-0".to_string());
		let name = name.unwrap_or(&default_name);

		let path = std::path::Path::new(name);
		let path = if path.is_relative() {
			std::path::Path::new(&std::env::var("XDG_RUNTIME_DIR").unwrap())
				.join(path)
		} else {
			path.to_path_buf()
		};

		let socket = UnixSocket::connect(path);

		WaylandUnixSocket { socket }
	}

	pub fn read_event(&mut self) -> Vec<(EventHeader, std::vec::Vec<u8>)> {
		self.socket.read_event()
	}
}

impl ReadEvent for WaylandUnixSocket {
	fn read_event(&mut self) -> Vec<(EventHeader, Vec<u8>)> {
		self.socket.read_event()
	}
}

impl WaylandSocket for WaylandUnixSocket {
	fn disconnect(&mut self) {
		self.socket.shutdown();
	}

	#[tracing::instrument(skip(self))]
	fn send(&mut self, buffer: &[u8], fd: &[RawFd]) {
		self.socket.write(buffer, fd);
	}

	fn connect(name: Option<&str>) -> Self
	where
		Self: Sized,
	{
		Self::connect(name)
	}
}

impl ReadEvent for UnixSocket {
	fn read_event(&mut self) -> Vec<(EventHeader, Vec<u8>)> {
		let mut buffer: [u8; 1024] = [0; 1024];
		let mut fds: [u8; 24] = [0; 24];
		let (size, _) = self.read(&mut buffer, &mut fds);
		if size == 1024 {
			warn!("Buffer is full");
		}
		let mut ret_value = Vec::new();
		let mut read_size: usize = 0;
		while read_size < size {
			let mut event_header: [u8; size_of::<EventHeaderPre>()] =
				[0; size_of::<EventHeaderPre>()];
			unsafe {
				std::ptr::copy(
					&buffer[read_size] as *const u8,
					event_header.as_mut_ptr(),
					size_of::<EventHeaderPre>(),
				);
			}
			let event_header = unsafe {
				transmute::<[u8; size_of::<EventHeaderPre>()], EventHeaderPre>(
					event_header,
				)
				.convert_to_event_header()
			};
			let msg_size = event_header.msg_size as usize;
			let mut msg_body = vec![0; event_header.msg_size as usize];
			unsafe {
				std::ptr::copy(
					&buffer[read_size + size_of::<EventHeaderPre>()]
						as *const u8,
					msg_body.as_mut_ptr(),
					msg_size,
				);
			}
			ret_value.push((event_header, msg_body));
			read_size += size_of::<EventHeaderPre>() + msg_size;
		}
		return ret_value;
	}
}
