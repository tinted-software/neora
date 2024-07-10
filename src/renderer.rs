use std::collections::HashSet;

use anyhow::{Context as _, Result};
use raw_window_handle::{HasDisplayHandle as _, HasWindowHandle as _};
use vulkanalia::{
	loader::{LibloadingLoader, LIBRARY},
	vk::{
		self, DeviceV1_0 as _, Handle as _, HasBuilder, InstanceV1_0 as _,
		KhrSurfaceExtension, KhrSwapchainExtension, PolygonMode,
	},
	Device, Entry, Instance,
};

const MAX_FRAMES_IN_FLIGHT: usize = 2;
const VALIDATION_LAYER: vk::ExtensionName =
	vk::ExtensionName::from_bytes(b"VK_LAYER_KHRONOS_validation");

pub struct Renderer {
	pub instance: Instance,
	pub physical_device: vk::PhysicalDevice,
	pub device: Device,
	pub graphics_queue: vk::Queue,
	pub present_queue: vk::Queue,
	pub queue_family_indices: QueueFamilyIndices,
	pub surface: vk::SurfaceKHR,
	pub swapchain: vk::SwapchainKHR,
	pub swapchain_images: Vec<vk::Image>,
	pub swapchain_format: vk::Format,
	pub swapchain_extent: vk::Extent2D,
	pub swapchain_image_views: Vec<vk::ImageView>,
	pub vertex_shader_module: vk::ShaderModule,
	pub fragment_shader_module: vk::ShaderModule,
	pub pipeline_layout: vk::PipelineLayout,
	pub render_pass: vk::RenderPass,
	pub pipeline: vk::Pipeline,
	pub framebuffers: Vec<vk::Framebuffer>,
	pub command_pool: vk::CommandPool,
	pub command_buffers: Vec<vk::CommandBuffer>,
	pub image_available_semaphores: Vec<vk::Semaphore>,
	pub render_finished_semaphores: Vec<vk::Semaphore>,
	pub frame: usize,
	pub in_flight_fences: Vec<vk::Fence>,
	pub images_in_flight: Vec<vk::Fence>,
}

impl Renderer {
	/// # Safety
	pub unsafe fn new(window: &winit::window::Window) -> Result<Self> {
		let application_info = vk::ApplicationInfo::builder()
			.application_name(b"macOS but it FUCKING SUCKS\0")
			.application_version(vk::make_version(1, 0, 0))
			.engine_name(b"No Engine\0")
			.engine_version(vk::make_version(1, 0, 0))
			.api_version(vk::make_version(1, 0, 0));

		let extensions = vec![
			vk::KHR_SURFACE_EXTENSION.name.as_ptr(),
			vk::KHR_WAYLAND_SURFACE_EXTENSION.name.as_ptr(),
			vk::KHR_DISPLAY_EXTENSION.name.as_ptr(),
		];
		let layers = vec![VALIDATION_LAYER.as_ptr()];

		let flags = vk::InstanceCreateFlags::empty();

		let info = vk::InstanceCreateInfo::builder()
			.application_info(&application_info)
			.enabled_layer_names(&layers)
			.enabled_extension_names(&extensions)
			.flags(flags);

		let loader = LibloadingLoader::new(LIBRARY)?;
		let entry = Entry::new(loader).map_err(|e| {
			anyhow::anyhow!("Failed to load vulkan library: {}", e.to_string())
		})?;
		let instance = entry.create_instance(&info, None)?;

		let mut physical_device = None;

		for device in instance.enumerate_physical_devices()? {
			let properties = instance.get_physical_device_properties(device);
			tracing::debug!("Physical device: {}", properties.device_name);

			physical_device = Some(device);
		}

		let physical_device =
			physical_device.context("No physical devices found.")?;

		let mut queue_family_indices =
			QueueFamilyIndices::get(&instance, physical_device)?;

		let window_handle = window.window_handle()?;
		let display_handle = window.display_handle()?;

		let surface = vulkanalia::window::create_surface(
			&instance,
			&display_handle,
			&window_handle,
		)
		.context("Failed to create surface.")?;

		let support =
			SwapchainSupport::get(&instance, surface, physical_device)?;
		if support.formats.is_empty() || support.present_modes.is_empty() {
			return Err(anyhow::anyhow!("Insufficient swapchain support."));
		}

		let properties = instance
			.get_physical_device_queue_family_properties(physical_device);

		let mut present = None;
		for (index, _properties) in properties.iter().enumerate() {
			if instance.get_physical_device_surface_support_khr(
				physical_device,
				index as u32,
				surface,
			)? {
				present = Some(index as u32);
				break;
			}
		}

		queue_family_indices.present = present;

		if queue_family_indices.present.is_none() {
			anyhow::bail!("Missing required queue families.")
		}

		let mut unique_indices = HashSet::new();
		unique_indices.insert(queue_family_indices.graphics);
		unique_indices.insert(queue_family_indices.present.unwrap());

		let extensions = vec![vk::KHR_SWAPCHAIN_EXTENSION.name.as_ptr()];

		let queue_priorities = &[1.0];
		let queue_infos = unique_indices
			.iter()
			.map(|i| {
				vk::DeviceQueueCreateInfo::builder()
					.queue_family_index(*i)
					.queue_priorities(queue_priorities)
			})
			.collect::<Vec<_>>();

		let features = vk::PhysicalDeviceFeatures::builder();
		let info = vk::DeviceCreateInfo::builder()
			.queue_create_infos(&queue_infos)
			.enabled_layer_names(&layers)
			.enabled_extension_names(&extensions)
			.enabled_features(&features);
		let device = instance.create_device(physical_device, &info, None)?;
		let graphics_queue =
			device.get_device_queue(queue_family_indices.graphics, 0);
		let present_queue =
			device.get_device_queue(queue_family_indices.present.unwrap(), 0);

		let support =
			SwapchainSupport::get(&instance, surface, physical_device)?;

		let surface_format = get_swapchain_surface_format(&support.formats);
		let present_mode = get_swapchain_present_mode(&support.present_modes);
		let swapchain_extent =
			get_swapchain_extent(window, support.capabilities);
		let mut image_count = support.capabilities.min_image_count + 1;
		if support.capabilities.max_image_count != 0
			&& image_count > support.capabilities.max_image_count
		{
			image_count = support.capabilities.max_image_count;
		}

		let mut indices = vec![];
		let image_sharing_mode = if queue_family_indices.graphics
			!= queue_family_indices.present.unwrap()
		{
			indices.push(queue_family_indices.graphics);
			indices.push(queue_family_indices.present.unwrap());
			vk::SharingMode::CONCURRENT
		} else {
			vk::SharingMode::EXCLUSIVE
		};

		let info = vk::SwapchainCreateInfoKHR::builder()
			.surface(surface)
			.min_image_count(image_count)
			.image_format(surface_format.format)
			.image_color_space(surface_format.color_space)
			.image_extent(swapchain_extent)
			.image_array_layers(1)
			.image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT)
			.image_sharing_mode(image_sharing_mode)
			.queue_family_indices(&indices)
			.pre_transform(support.capabilities.current_transform)
			.composite_alpha(vk::CompositeAlphaFlagsKHR::OPAQUE)
			.present_mode(present_mode)
			.clipped(true)
			.old_swapchain(vk::SwapchainKHR::null());
		let swapchain = device.create_swapchain_khr(&info, None)?;

		let swapchain_images = device.get_swapchain_images_khr(swapchain)?;

		let swapchain_image_views = swapchain_images
			.iter()
			.map(|i| {
				let components = vk::ComponentMapping::builder()
					.r(vk::ComponentSwizzle::IDENTITY)
					.g(vk::ComponentSwizzle::IDENTITY)
					.b(vk::ComponentSwizzle::IDENTITY)
					.a(vk::ComponentSwizzle::IDENTITY);

				let subresource_range = vk::ImageSubresourceRange::builder()
					.aspect_mask(vk::ImageAspectFlags::COLOR)
					.base_mip_level(0)
					.level_count(1)
					.base_array_layer(0)
					.layer_count(1);

				let info = vk::ImageViewCreateInfo::builder()
					.image(*i)
					.view_type(vk::ImageViewType::_2D)
					.format(surface_format.format)
					.components(components)
					.subresource_range(subresource_range);

				device.create_image_view(&info, None)
			})
			.collect::<Result<Vec<_>, _>>()?;

		let vert = include_bytes!("./shader.vertex.spv");
		let frag = include_bytes!("./shader.fragment.spv");

		let vertex_shader_module = create_shader_module(&device, &vert[..])?;
		let fragment_shader_module = create_shader_module(&device, &frag[..])?;

		let vert_stage = vk::PipelineShaderStageCreateInfo::builder()
			.stage(vk::ShaderStageFlags::VERTEX)
			.module(vertex_shader_module)
			.name(b"main\0");

		let frag_stage = vk::PipelineShaderStageCreateInfo::builder()
			.stage(vk::ShaderStageFlags::FRAGMENT)
			.module(fragment_shader_module)
			.name(b"main\0");

		let vertex_input_state =
			vk::PipelineVertexInputStateCreateInfo::builder();

		let input_assembly_state =
			vk::PipelineInputAssemblyStateCreateInfo::builder()
				.topology(vk::PrimitiveTopology::TRIANGLE_LIST)
				.primitive_restart_enable(false);

		let viewport = vk::Viewport::builder()
			.x(0.0)
			.y(0.0)
			.width(swapchain_extent.width as f32)
			.height(swapchain_extent.height as f32)
			.min_depth(0.0)
			.max_depth(1.0);

		let scissor = vk::Rect2D::builder()
			.offset(vk::Offset2D { x: 0, y: 0 })
			.extent(swapchain_extent);

		let viewports = &[viewport];
		let scissors = &[scissor];
		let viewport_state = vk::PipelineViewportStateCreateInfo::builder()
			.viewports(viewports)
			.scissors(scissors);

		let rasterization_state =
			vk::PipelineRasterizationStateCreateInfo::builder()
				.depth_clamp_enable(false)
				.rasterizer_discard_enable(false)
				.polygon_mode(PolygonMode::FILL)
				.line_width(1.0)
				.cull_mode(vk::CullModeFlags::BACK)
				.front_face(vk::FrontFace::CLOCKWISE)
				.depth_bias_enable(false);

		let multisample_state =
			vk::PipelineMultisampleStateCreateInfo::builder()
				.sample_shading_enable(false)
				.rasterization_samples(vk::SampleCountFlags::_1);

		let attachment = vk::PipelineColorBlendAttachmentState::builder()
			.color_write_mask(vk::ColorComponentFlags::all())
			.blend_enable(true)
			.src_color_blend_factor(vk::BlendFactor::SRC_ALPHA)
			.dst_color_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
			.color_blend_op(vk::BlendOp::ADD)
			.src_alpha_blend_factor(vk::BlendFactor::ONE)
			.dst_alpha_blend_factor(vk::BlendFactor::ZERO)
			.alpha_blend_op(vk::BlendOp::ADD);

		let attachments = &[attachment];
		let color_blend_state =
			vk::PipelineColorBlendStateCreateInfo::builder()
				.logic_op_enable(false)
				.logic_op(vk::LogicOp::COPY)
				.attachments(attachments)
				.blend_constants([0.0, 0.0, 0.0, 0.0]);

		let dynamic_states =
			&[vk::DynamicState::VIEWPORT, vk::DynamicState::LINE_WIDTH];

		let _dynamic_state = vk::PipelineDynamicStateCreateInfo::builder()
			.dynamic_states(dynamic_states);

		let layout_info = vk::PipelineLayoutCreateInfo::builder();

		let pipeline_layout =
			device.create_pipeline_layout(&layout_info, None)?;

		// Render pass
		let color_attachment = vk::AttachmentDescription::builder()
			.format(surface_format.format)
			.samples(vk::SampleCountFlags::_1)
			.load_op(vk::AttachmentLoadOp::CLEAR)
			.store_op(vk::AttachmentStoreOp::STORE)
			.stencil_load_op(vk::AttachmentLoadOp::DONT_CARE)
			.stencil_store_op(vk::AttachmentStoreOp::DONT_CARE)
			.initial_layout(vk::ImageLayout::UNDEFINED)
			.final_layout(vk::ImageLayout::PRESENT_SRC_KHR);

		let color_attachment_ref = vk::AttachmentReference::builder()
			.attachment(0)
			.layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL);

		let color_attachments = &[color_attachment_ref];
		let subpass = vk::SubpassDescription::builder()
			.pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
			.color_attachments(color_attachments);

		let dependency = vk::SubpassDependency::builder()
			.src_subpass(vk::SUBPASS_EXTERNAL)
			.dst_subpass(0)
			.src_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
			.src_access_mask(vk::AccessFlags::empty())
			.dst_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
			.dst_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE);

		let attachments = &[color_attachment];
		let subpasses = &[subpass];
		let dependencies = &[dependency];
		let info = vk::RenderPassCreateInfo::builder()
			.attachments(attachments)
			.subpasses(subpasses)
			.dependencies(dependencies);

		let render_pass = device.create_render_pass(&info, None)?;

		let stages = &[vert_stage, frag_stage];
		let info = vk::GraphicsPipelineCreateInfo::builder()
			.stages(stages)
			.vertex_input_state(&vertex_input_state)
			.input_assembly_state(&input_assembly_state)
			.viewport_state(&viewport_state)
			.rasterization_state(&rasterization_state)
			.multisample_state(&multisample_state)
			.color_blend_state(&color_blend_state)
			.layout(pipeline_layout)
			.render_pass(render_pass)
			.base_pipeline_handle(vk::Pipeline::null())
			.base_pipeline_index(-1)
			.subpass(0);

		let pipeline = device
			.create_graphics_pipelines(
				vk::PipelineCache::null(),
				&[info],
				None,
			)?
			.0[0];

		let framebuffers = swapchain_image_views
			.iter()
			.map(|i| {
				let attachments = &[*i];
				let create_info = vk::FramebufferCreateInfo::builder()
					.render_pass(render_pass)
					.attachments(attachments)
					.width(swapchain_extent.width)
					.height(swapchain_extent.height)
					.layers(1);

				device.create_framebuffer(&create_info, None)
			})
			.collect::<Result<Vec<_>, _>>()?;

		let info = vk::CommandPoolCreateInfo::builder()
			.flags(vk::CommandPoolCreateFlags::empty())
			.queue_family_index(queue_family_indices.graphics);

		let command_pool = device.create_command_pool(&info, None)?;

		let allocate_info = vk::CommandBufferAllocateInfo::builder()
			.command_pool(command_pool)
			.level(vk::CommandBufferLevel::PRIMARY)
			.command_buffer_count(framebuffers.len() as u32);

		let command_buffers =
			device.allocate_command_buffers(&allocate_info)?;

		for (i, command_buffer) in command_buffers.iter().enumerate() {
			let info = vk::CommandBufferBeginInfo::builder();

			device.begin_command_buffer(*command_buffer, &info)?;

			let render_area = vk::Rect2D::builder()
				.offset(vk::Offset2D::default())
				.extent(swapchain_extent);

			let color_clear_value = vk::ClearValue {
				color: vk::ClearColorValue {
					float32: [0.0, 0.0, 0.0, 1.0],
				},
			};

			let clear_values = &[color_clear_value];
			let info = vk::RenderPassBeginInfo::builder()
				.render_pass(render_pass)
				.framebuffer(framebuffers[i])
				.render_area(render_area)
				.clear_values(clear_values);

			device.cmd_begin_render_pass(
				*command_buffer,
				&info,
				vk::SubpassContents::INLINE,
			);
			device.cmd_bind_pipeline(
				*command_buffer,
				vk::PipelineBindPoint::GRAPHICS,
				pipeline,
			);
			device.cmd_draw(*command_buffer, 3, 1, 0, 0);
			device.cmd_end_render_pass(*command_buffer);

			device.end_command_buffer(*command_buffer)?;
		}

		let semaphore_info = vk::SemaphoreCreateInfo::builder();
		let fence_info = vk::FenceCreateInfo::builder()
			.flags(vk::FenceCreateFlags::SIGNALED);

		let mut image_available_semaphores = Vec::new();
		let mut render_finished_semaphores = Vec::new();
		let mut in_flight_fences = Vec::new();

		for _ in 0..MAX_FRAMES_IN_FLIGHT {
			image_available_semaphores
				.push(device.create_semaphore(&semaphore_info, None)?);
			render_finished_semaphores
				.push(device.create_semaphore(&semaphore_info, None)?);
			in_flight_fences.push(device.create_fence(&fence_info, None)?);
		}

		let images_in_flight =
			swapchain_images.iter().map(|_| vk::Fence::null()).collect();

		Ok(Self {
			instance,
			device,
			physical_device,
			graphics_queue,
			present_queue,
			queue_family_indices,
			surface,
			swapchain,
			swapchain_images,
			swapchain_image_views,
			swapchain_format: surface_format.format,
			swapchain_extent,
			fragment_shader_module,
			vertex_shader_module,
			pipeline_layout,
			render_pass,
			pipeline,
			framebuffers,
			command_pool,
			command_buffers,
			image_available_semaphores,
			render_finished_semaphores,
			in_flight_fences,
			images_in_flight,
			frame: 0,
		})
	}

	/// # Safety
	pub unsafe fn render_frame(&mut self) -> Result<()> {
		let in_flight_fence = self.in_flight_fences[self.frame];

		self.device
			.wait_for_fences(&[in_flight_fence], true, u64::MAX)?;

		let image_index = self
			.device
			.acquire_next_image_khr(
				self.swapchain,
				u64::MAX,
				self.image_available_semaphores[self.frame],
				vk::Fence::null(),
			)?
			.0 as usize;

		let image_in_flight = self.images_in_flight[image_index];
		if !image_in_flight.is_null() {
			self.device
				.wait_for_fences(&[image_in_flight], true, u64::MAX)?;
		}

		self.images_in_flight[image_index] = in_flight_fence;

		let wait_semaphores = &[self.image_available_semaphores[self.frame]];
		let wait_stages = &[vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT];
		let command_buffers = &[self.command_buffers[image_index]];
		let signal_semaphores = &[self.render_finished_semaphores[self.frame]];
		let submit_info = vk::SubmitInfo::builder()
			.wait_semaphores(wait_semaphores)
			.wait_dst_stage_mask(wait_stages)
			.command_buffers(command_buffers)
			.signal_semaphores(signal_semaphores);

		self.device.reset_fences(&[in_flight_fence])?;

		self.device.queue_submit(
			self.graphics_queue,
			&[submit_info],
			in_flight_fence,
		)?;

		let swapchains = &[self.swapchain];
		let image_indices = &[image_index as u32];
		let present_info = vk::PresentInfoKHR::builder()
			.wait_semaphores(signal_semaphores)
			.swapchains(swapchains)
			.image_indices(image_indices);

		self.device
			.queue_present_khr(self.present_queue, &present_info)?;

		self.frame = (self.frame + 1) % MAX_FRAMES_IN_FLIGHT;

		Ok(())
	}
}

impl Drop for Renderer {
	fn drop(&mut self) {
		unsafe {
			self.device.device_wait_idle().unwrap();

			self.in_flight_fences
				.iter()
				.for_each(|f| self.device.destroy_fence(*f, None));
			self.render_finished_semaphores
				.iter()
				.for_each(|s| self.device.destroy_semaphore(*s, None));
			self.image_available_semaphores
				.iter()
				.for_each(|s| self.device.destroy_semaphore(*s, None));
			self.device.destroy_command_pool(self.command_pool, None);
			self.framebuffers
				.iter()
				.for_each(|f| self.device.destroy_framebuffer(*f, None));
			self.device.destroy_pipeline(self.pipeline, None);
			self.device
				.destroy_pipeline_layout(self.pipeline_layout, None);
			self.device.destroy_render_pass(self.render_pass, None);
			self.swapchain_image_views
				.iter()
				.for_each(|v| self.device.destroy_image_view(*v, None));
			self.device.destroy_swapchain_khr(self.swapchain, None);
			self.device.destroy_device(None);
			self.instance.destroy_surface_khr(self.surface, None);

			self.instance.destroy_instance(None);
		}
	}
}

/// # Safety
pub unsafe fn create_shader_module(
	device: &Device,
	bytecode: &[u8],
) -> Result<vk::ShaderModule> {
	use vulkanalia::bytecode::Bytecode;
	let bytecode = Bytecode::new(bytecode).unwrap();

	let info = vk::ShaderModuleCreateInfo::builder()
		.code_size(bytecode.code_size())
		.code(bytecode.code());

	Ok(device.create_shader_module(&info, None)?)
}

#[derive(Copy, Clone, Debug)]
pub struct QueueFamilyIndices {
	pub graphics: u32,
	pub present: Option<u32>,
}

impl QueueFamilyIndices {
	unsafe fn get(
		instance: &Instance,
		physical_device: vk::PhysicalDevice,
	) -> Result<Self> {
		let properties = instance
			.get_physical_device_queue_family_properties(physical_device);

		let graphics = properties
			.iter()
			.position(|p| p.queue_flags.contains(vk::QueueFlags::GRAPHICS))
			.map(|i| i as u32);

		if let Some(graphics) = graphics {
			Ok(Self {
				graphics,
				present: None,
			})
		} else {
			Err(anyhow::anyhow!("Missing required queue families."))
		}
	}
}

#[derive(Clone, Debug)]
pub struct SwapchainSupport {
	pub capabilities: vk::SurfaceCapabilitiesKHR,
	pub formats: Vec<vk::SurfaceFormatKHR>,
	pub present_modes: Vec<vk::PresentModeKHR>,
}

impl SwapchainSupport {
	/// # Safety
	pub unsafe fn get(
		instance: &Instance,
		surface: vk::SurfaceKHR,
		physical_device: vk::PhysicalDevice,
	) -> Result<Self> {
		Ok(Self {
			capabilities: instance
				.get_physical_device_surface_capabilities_khr(
					physical_device,
					surface,
				)?,
			formats: instance.get_physical_device_surface_formats_khr(
				physical_device,
				surface,
			)?,
			present_modes: instance
				.get_physical_device_surface_present_modes_khr(
					physical_device,
					surface,
				)?,
		})
	}
}

pub fn get_swapchain_surface_format(
	formats: &[vk::SurfaceFormatKHR],
) -> vk::SurfaceFormatKHR {
	formats
		.iter()
		.cloned()
		.find(|f| {
			f.format == vk::Format::B8G8R8A8_SRGB
				&& f.color_space == vk::ColorSpaceKHR::SRGB_NONLINEAR
		})
		.unwrap_or_else(|| formats[0])
}

pub fn get_swapchain_present_mode(
	present_modes: &[vk::PresentModeKHR],
) -> vk::PresentModeKHR {
	present_modes
		.iter()
		.cloned()
		.find(|m| *m == vk::PresentModeKHR::MAILBOX)
		.unwrap_or(vk::PresentModeKHR::FIFO)
}

pub fn get_swapchain_extent(
	window: &winit::window::Window,
	capabilities: vk::SurfaceCapabilitiesKHR,
) -> vk::Extent2D {
	if capabilities.current_extent.width != u32::MAX {
		capabilities.current_extent
	} else {
		vk::Extent2D::builder()
			.width(window.inner_size().width.clamp(
				capabilities.min_image_extent.width,
				capabilities.max_image_extent.width,
			))
			.height(window.inner_size().height.clamp(
				capabilities.min_image_extent.height,
				capabilities.max_image_extent.height,
			))
			.build()
	}
}
