use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

fn main() {
	let out_path = PathBuf::from(std::env::var("OUT_DIR").unwrap());

	let dest_path = out_path.join("wayland.rs");
	let mut file = File::create(&dest_path).unwrap();

	file.write_all(
		neora_protocol_generator::generate_wayland_protocol_code().as_bytes(),
	)
	.unwrap();

	Command::new("rustfmt").arg(dest_path).spawn().unwrap();
}
