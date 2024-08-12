use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Entry {
	#[serde(rename = "@name")]
	pub name: String,
	#[serde(rename = "@value")]
	pub value: String,
	#[serde(rename = "@summary")]
	pub summary: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EnumChild {
	Description(String),
	Entry(Entry),
}

#[derive(Debug, Deserialize)]
pub struct Enum {
	#[serde(rename = "@name")]
	pub name: String,
	#[serde(rename = "$value", default)]
	pub items: Vec<EnumChild>,
}

#[derive(Debug, Deserialize)]
pub struct Arg {
	#[serde(rename = "@name")]
	pub name: String,

	#[serde(rename = "@type", default)]
	pub typ: String,

	#[serde(rename = "@summary")]
	pub summary: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EventOrRequestField {
	Description(String),
	Arg(Arg),
}

#[derive(Debug, Deserialize)]
pub struct Event {
	#[serde(rename = "@name")]
	pub name: String,

	#[serde(rename = "$value", default)]
	pub items: Vec<EventOrRequestField>,
}

#[derive(Debug, Deserialize)]
pub struct Request {
	#[serde(rename = "@name")]
	pub name: String,

	#[serde(rename = "$value", default)]
	pub items: Vec<EventOrRequestField>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InterfaceChild {
	Request(Request),
	Description(String),
	Event(Event),
	Enum(Enum),
}

#[derive(Debug, Deserialize)]
pub struct Interface {
	#[serde(rename = "@name")]
	pub name: String,
	#[serde(rename = "@version")]
	pub version: String,

	#[serde(rename = "$value", default)]
	pub items: Vec<InterfaceChild>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProtocolChild {
	Interface(Interface),
	CopyRight(String),
}

#[derive(Debug, Deserialize)]
pub struct Protocol {
	#[serde(rename = "@name")]
	pub name: String,

	#[serde(rename = "$value", default)]
	pub items: Vec<ProtocolChild>,
}

pub fn parse_wayland_protocol() -> Protocol {
	let contents = include_str!("../../../protocols/wayland.xml");

	quick_xml::de::from_str(&contents).unwrap()
}
