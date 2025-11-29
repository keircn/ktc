use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, RawFd};

use ash::vk;
use drm::control::{crtc, connector, framebuffer, Device as ControlDevice};
use gbm::{AsRaw, Device as GbmDevice};

struct DrmCard(std::fs::File);

impl std::os::fd::AsFd for DrmCard {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.0.as_fd()
    }
}

impl drm::Device for DrmCard {}
impl ControlDevice for DrmCard {}

#[derive(Clone, Debug)]
pub struct DmaBufFormat {
    pub format: u32,
    pub modifier: u64,
}

pub struct VulkanRenderer {
    _entry: ash::Entry,
    instance: ash::Instance,
    physical_device: vk::PhysicalDevice,
    device: ash::Device,
    queue: vk::Queue,
    queue_family_index: u32,
    command_pool: vk::CommandPool,
    command_buffer: vk::CommandBuffer,
    render_pass: vk::RenderPass,
    pipeline_layout: vk::PipelineLayout,
    texture_pipeline: vk::Pipeline,
    color_pipeline: vk::Pipeline,
    descriptor_set_layout: vk::DescriptorSetLayout,
    descriptor_pool: vk::DescriptorPool,
    sampler: vk::Sampler,
    drm_card: DrmCard,
    gbm: GbmDevice<std::fs::File>,
    crtc: crtc::Handle,
    connector: connector::Handle,
    mode: drm::control::Mode,
    render_images: Vec<RenderTarget>,
    current_target: usize,
    mode_set: bool,
    width: u32,
    height: u32,
    physical_width: u32,
    physical_height: u32,
    shm_textures: HashMap<u64, VulkanTexture>,
    dmabuf_textures: HashMap<u64, VulkanTexture>,
    pub supported_formats: Vec<DmaBufFormat>,
    external_memory_fd: ash::khr::external_memory_fd::Device,
}

struct RenderTarget {
    gbm_bo: *mut gbm_sys::gbm_bo,
    gbm_ptr: *mut u8,
    gbm_stride: u32,
    gbm_map_data: *mut std::ffi::c_void,
    image: vk::Image,
    memory: vk::DeviceMemory,
    view: vk::ImageView,
    framebuffer: vk::Framebuffer,
    drm_fb: Option<framebuffer::Handle>,
    staging_buffer: vk::Buffer,
    staging_memory: vk::DeviceMemory,
    staging_ptr: *mut u8,
}

struct VulkanTexture {
    image: vk::Image,
    memory: vk::DeviceMemory,
    view: vk::ImageView,
    descriptor_set: vk::DescriptorSet,
    width: u32,
    height: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct PushConstants {
    offset: [f32; 2],
    size: [f32; 2],
    screen_size: [f32; 2],
    _padding: [f32; 2],
    color: [f32; 4],
}

impl VulkanRenderer {
    pub fn new(drm_device: std::fs::File) -> Result<Self, Box<dyn std::error::Error>> {
        Self::new_with_config(drm_device, None, true)
    }
    
    pub fn new_with_config(
        drm_device: std::fs::File,
        preferred_mode: Option<(u16, u16, Option<u32>)>,
        _vsync: bool,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let gbm = GbmDevice::new(drm_device.try_clone()?)?;
        let card = DrmCard(drm_device.try_clone()?);
        
        let res = card.resource_handles()?;
        let connectors: Vec<_> = res.connectors().iter()
            .filter_map(|&conn| card.get_connector(conn, true).ok())
            .collect();
        
        let connector_info = connectors.iter()
            .find(|c| c.state() == drm::control::connector::State::Connected)
            .ok_or("No connected display found")?;
        
        let connector_handle = connector_info.handle();
        
        log::info!("[vulkan] Available display modes:");
        for m in connector_info.modes() {
            let (w, h) = m.size();
            log::info!("[vulkan]   {}x{}@{}Hz", w, h, m.vrefresh());
        }
        
        let mode = if let Some((pref_w, pref_h, pref_refresh)) = preferred_mode {
            connector_info.modes().iter()
                .find(|m| {
                    let (w, h) = m.size();
                    let matches_res = w == pref_w && h == pref_h;
                    if let Some(refresh) = pref_refresh {
                        matches_res && m.vrefresh() == refresh
                    } else {
                        matches_res
                    }
                })
                .or_else(|| connector_info.modes().first())
                .copied()
                .ok_or("No display mode available")?
        } else {
            *connector_info.modes().first().ok_or("No display mode available")?
        };
        
        let (width, height) = mode.size();
        let width = width as u32;
        let height = height as u32;
        
        let (physical_width, physical_height) = connector_info.size().unwrap_or((0, 0));
        log::info!("[vulkan] Physical size: {}x{}mm", physical_width, physical_height);
        
        let crtc_handle = res.crtcs().first().copied().ok_or("No CRTC available")?;
        
        log::info!("[vulkan] Selected mode: {}x{}@{}Hz", width, height, mode.vrefresh());
        
        let entry = unsafe { ash::Entry::load()? };
        
        let app_name = CString::new("ktc")?;
        let engine_name = CString::new("ktc")?;
        
        let app_info = vk::ApplicationInfo::default()
            .application_name(&app_name)
            .application_version(vk::make_api_version(0, 0, 1, 0))
            .engine_name(&engine_name)
            .engine_version(vk::make_api_version(0, 0, 1, 0))
            .api_version(vk::API_VERSION_1_2);
        
        let extensions = [
            ash::khr::external_memory_capabilities::NAME.as_ptr(),
            ash::khr::get_physical_device_properties2::NAME.as_ptr(),
        ];
        
        let create_info = vk::InstanceCreateInfo::default()
            .application_info(&app_info)
            .enabled_extension_names(&extensions);
        
        let instance = unsafe { entry.create_instance(&create_info, None)? };
        
        log::info!("[vulkan] Instance created");
        
        let physical_devices = unsafe { instance.enumerate_physical_devices()? };
        let physical_device = physical_devices.into_iter()
            .find(|&pd| {
                let props = unsafe { instance.get_physical_device_properties(pd) };
                props.device_type == vk::PhysicalDeviceType::DISCRETE_GPU ||
                props.device_type == vk::PhysicalDeviceType::INTEGRATED_GPU
            })
            .ok_or("No suitable GPU found")?;
        
        let props = unsafe { instance.get_physical_device_properties(physical_device) };
        let device_name = unsafe { CStr::from_ptr(props.device_name.as_ptr()) };
        log::info!("[vulkan] Using device: {:?}", device_name);
        
        let queue_families = unsafe { instance.get_physical_device_queue_family_properties(physical_device) };
        let queue_family_index = queue_families.iter()
            .position(|qf| qf.queue_flags.contains(vk::QueueFlags::GRAPHICS))
            .ok_or("No graphics queue family")? as u32;
        
        let queue_priorities = [1.0f32];
        let queue_create_info = vk::DeviceQueueCreateInfo::default()
            .queue_family_index(queue_family_index)
            .queue_priorities(&queue_priorities);
        
        let available_extensions = unsafe { instance.enumerate_device_extension_properties(physical_device)? };
        let available_ext_names: Vec<_> = available_extensions.iter()
            .filter_map(|ext| {
                let name = unsafe { CStr::from_ptr(ext.extension_name.as_ptr()) };
                name.to_str().ok().map(|s| s.to_string())
            })
            .collect();
        
        log::info!("[vulkan] Available device extensions: {}", available_ext_names.len());
        
        let mut device_extensions: Vec<*const i8> = Vec::new();
        
        let ext_external_memory_fd = ash::khr::external_memory_fd::NAME.to_str().unwrap_or("");
        let ext_external_memory_dmabuf = ash::ext::external_memory_dma_buf::NAME.to_str().unwrap_or("");
        let ext_image_drm_format_modifier = ash::ext::image_drm_format_modifier::NAME.to_str().unwrap_or("");
        
        let has_external_memory_fd = available_ext_names.iter().any(|e| e == ext_external_memory_fd);
        let has_external_memory_dmabuf = available_ext_names.iter().any(|e| e == ext_external_memory_dmabuf);
        let has_image_drm_format_modifier = available_ext_names.iter().any(|e| e == ext_image_drm_format_modifier);
        
        log::info!("[vulkan] Extension support: external_memory_fd={}, external_memory_dma_buf={}, image_drm_format_modifier={}",
            has_external_memory_fd, has_external_memory_dmabuf, has_image_drm_format_modifier);
        
        if has_external_memory_fd {
            device_extensions.push(ash::khr::external_memory_fd::NAME.as_ptr());
        }
        if has_external_memory_dmabuf {
            device_extensions.push(ash::ext::external_memory_dma_buf::NAME.as_ptr());
        }
        if has_image_drm_format_modifier {
            device_extensions.push(ash::ext::image_drm_format_modifier::NAME.as_ptr());
        }
        
        if !has_external_memory_fd || !has_external_memory_dmabuf {
            return Err("Missing required Vulkan extensions for DMA-BUF import".into());
        }
        
        let device_create_info = vk::DeviceCreateInfo::default()
            .queue_create_infos(std::slice::from_ref(&queue_create_info))
            .enabled_extension_names(&device_extensions);
        
        let device = unsafe { instance.create_device(physical_device, &device_create_info, None)? };
        let queue = unsafe { device.get_device_queue(queue_family_index, 0) };
        
        log::info!("[vulkan] Device created with {} extensions", device_extensions.len());
        
        let external_memory_fd = ash::khr::external_memory_fd::Device::new(&instance, &device);
        
        let pool_info = vk::CommandPoolCreateInfo::default()
            .queue_family_index(queue_family_index)
            .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER);
        let command_pool = unsafe { device.create_command_pool(&pool_info, None)? };
        
        let alloc_info = vk::CommandBufferAllocateInfo::default()
            .command_pool(command_pool)
            .level(vk::CommandBufferLevel::PRIMARY)
            .command_buffer_count(1);
        let command_buffer = unsafe { device.allocate_command_buffers(&alloc_info)? }[0];
        
        let attachment = vk::AttachmentDescription::default()
            .format(vk::Format::B8G8R8A8_UNORM)
            .samples(vk::SampleCountFlags::TYPE_1)
            .load_op(vk::AttachmentLoadOp::CLEAR)
            .store_op(vk::AttachmentStoreOp::STORE)
            .stencil_load_op(vk::AttachmentLoadOp::DONT_CARE)
            .stencil_store_op(vk::AttachmentStoreOp::DONT_CARE)
            .initial_layout(vk::ImageLayout::UNDEFINED)
            .final_layout(vk::ImageLayout::GENERAL);
        
        let attachment_ref = vk::AttachmentReference::default()
            .attachment(0)
            .layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL);
        
        let subpass = vk::SubpassDescription::default()
            .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
            .color_attachments(std::slice::from_ref(&attachment_ref));
        
        let render_pass_info = vk::RenderPassCreateInfo::default()
            .attachments(std::slice::from_ref(&attachment))
            .subpasses(std::slice::from_ref(&subpass));
        
        let render_pass = unsafe { device.create_render_pass(&render_pass_info, None)? };
        
        let sampler_binding = vk::DescriptorSetLayoutBinding::default()
            .binding(0)
            .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
            .descriptor_count(1)
            .stage_flags(vk::ShaderStageFlags::FRAGMENT);
        
        let layout_info = vk::DescriptorSetLayoutCreateInfo::default()
            .bindings(std::slice::from_ref(&sampler_binding));
        
        let descriptor_set_layout = unsafe { device.create_descriptor_set_layout(&layout_info, None)? };
        
        let push_constant_range = vk::PushConstantRange::default()
            .stage_flags(vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT)
            .offset(0)
            .size(std::mem::size_of::<PushConstants>() as u32);
        
        let pipeline_layout_info = vk::PipelineLayoutCreateInfo::default()
            .set_layouts(std::slice::from_ref(&descriptor_set_layout))
            .push_constant_ranges(std::slice::from_ref(&push_constant_range));
        
        let pipeline_layout = unsafe { device.create_pipeline_layout(&pipeline_layout_info, None)? };
        
        let sampler_info = vk::SamplerCreateInfo::default()
            .mag_filter(vk::Filter::LINEAR)
            .min_filter(vk::Filter::LINEAR)
            .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_EDGE)
            .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE)
            .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_EDGE);
        
        let sampler = unsafe { device.create_sampler(&sampler_info, None)? };
        
        let pool_size = vk::DescriptorPoolSize::default()
            .ty(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
            .descriptor_count(1000);
        
        let pool_info = vk::DescriptorPoolCreateInfo::default()
            .max_sets(1000)
            .pool_sizes(std::slice::from_ref(&pool_size))
            .flags(vk::DescriptorPoolCreateFlags::FREE_DESCRIPTOR_SET);
        
        let descriptor_pool = unsafe { device.create_descriptor_pool(&pool_info, None)? };
        
        let (texture_pipeline, color_pipeline) = Self::create_pipelines(
            &device,
            render_pass,
            pipeline_layout,
            width,
            height,
        )?;
        
        let render_images = Self::create_render_targets(
            &device,
            &instance,
            physical_device,
            &gbm,
            render_pass,
            width,
            height,
            &card,
        )?;
        
        let supported_formats = Self::query_dmabuf_formats();
        log::info!("[vulkan] Supported DMA-BUF formats: {}", supported_formats.len());
        
        log::info!("[vulkan] Renderer initialized: {}x{}", width, height);
        
        Ok(Self {
            _entry: entry,
            instance,
            physical_device,
            device,
            queue,
            queue_family_index,
            command_pool,
            command_buffer,
            render_pass,
            pipeline_layout,
            texture_pipeline,
            color_pipeline,
            descriptor_set_layout,
            descriptor_pool,
            sampler,
            drm_card: card,
            gbm,
            crtc: crtc_handle,
            connector: connector_handle,
            mode,
            render_images,
            current_target: 0,
            mode_set: false,
            width,
            height,
            physical_width,
            physical_height,
            shm_textures: HashMap::new(),
            dmabuf_textures: HashMap::new(),
            supported_formats,
            external_memory_fd,
        })
    }
    
    fn create_pipelines(
        device: &ash::Device,
        render_pass: vk::RenderPass,
        pipeline_layout: vk::PipelineLayout,
        width: u32,
        height: u32,
    ) -> Result<(vk::Pipeline, vk::Pipeline), Box<dyn std::error::Error>> {
        let vert_spv = include_bytes!("shaders/quad.vert.spv");
        let frag_texture_spv = include_bytes!("shaders/texture.frag.spv");
        let frag_color_spv = include_bytes!("shaders/color.frag.spv");
        
        let vert_module = Self::create_shader_module(device, vert_spv)?;
        let frag_texture_module = Self::create_shader_module(device, frag_texture_spv)?;
        let frag_color_module = Self::create_shader_module(device, frag_color_spv)?;
        
        let entry_point = CString::new("main")?;
        
        let vert_stage = vk::PipelineShaderStageCreateInfo::default()
            .stage(vk::ShaderStageFlags::VERTEX)
            .module(vert_module)
            .name(&entry_point);
        
        let frag_texture_stage = vk::PipelineShaderStageCreateInfo::default()
            .stage(vk::ShaderStageFlags::FRAGMENT)
            .module(frag_texture_module)
            .name(&entry_point);
        
        let frag_color_stage = vk::PipelineShaderStageCreateInfo::default()
            .stage(vk::ShaderStageFlags::FRAGMENT)
            .module(frag_color_module)
            .name(&entry_point);
        
        let vertex_input_info = vk::PipelineVertexInputStateCreateInfo::default();
        
        let input_assembly = vk::PipelineInputAssemblyStateCreateInfo::default()
            .topology(vk::PrimitiveTopology::TRIANGLE_LIST);
        
        let viewport = vk::Viewport {
            x: 0.0,
            y: 0.0,
            width: width as f32,
            height: height as f32,
            min_depth: 0.0,
            max_depth: 1.0,
        };
        
        let scissor = vk::Rect2D {
            offset: vk::Offset2D { x: 0, y: 0 },
            extent: vk::Extent2D { width, height },
        };
        
        let viewport_state = vk::PipelineViewportStateCreateInfo::default()
            .viewports(std::slice::from_ref(&viewport))
            .scissors(std::slice::from_ref(&scissor));
        
        let rasterizer = vk::PipelineRasterizationStateCreateInfo::default()
            .polygon_mode(vk::PolygonMode::FILL)
            .line_width(1.0)
            .cull_mode(vk::CullModeFlags::NONE)
            .front_face(vk::FrontFace::COUNTER_CLOCKWISE);
        
        let multisampling = vk::PipelineMultisampleStateCreateInfo::default()
            .rasterization_samples(vk::SampleCountFlags::TYPE_1);
        
        let color_blend_attachment = vk::PipelineColorBlendAttachmentState::default()
            .blend_enable(true)
            .src_color_blend_factor(vk::BlendFactor::ONE)
            .dst_color_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
            .color_blend_op(vk::BlendOp::ADD)
            .src_alpha_blend_factor(vk::BlendFactor::ONE)
            .dst_alpha_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
            .alpha_blend_op(vk::BlendOp::ADD)
            .color_write_mask(vk::ColorComponentFlags::RGBA);
        
        let color_blending = vk::PipelineColorBlendStateCreateInfo::default()
            .attachments(std::slice::from_ref(&color_blend_attachment));
        
        let texture_stages = [vert_stage, frag_texture_stage];
        let texture_pipeline_info = vk::GraphicsPipelineCreateInfo::default()
            .stages(&texture_stages)
            .vertex_input_state(&vertex_input_info)
            .input_assembly_state(&input_assembly)
            .viewport_state(&viewport_state)
            .rasterization_state(&rasterizer)
            .multisample_state(&multisampling)
            .color_blend_state(&color_blending)
            .layout(pipeline_layout)
            .render_pass(render_pass)
            .subpass(0);
        
        let color_stages = [vert_stage, frag_color_stage];
        let color_pipeline_info = vk::GraphicsPipelineCreateInfo::default()
            .stages(&color_stages)
            .vertex_input_state(&vertex_input_info)
            .input_assembly_state(&input_assembly)
            .viewport_state(&viewport_state)
            .rasterization_state(&rasterizer)
            .multisample_state(&multisampling)
            .color_blend_state(&color_blending)
            .layout(pipeline_layout)
            .render_pass(render_pass)
            .subpass(0);
        
        let pipelines = unsafe {
            device.create_graphics_pipelines(
                vk::PipelineCache::null(),
                &[texture_pipeline_info, color_pipeline_info],
                None,
            ).map_err(|e| format!("Failed to create pipelines: {:?}", e.1))?
        };
        
        unsafe {
            device.destroy_shader_module(vert_module, None);
            device.destroy_shader_module(frag_texture_module, None);
            device.destroy_shader_module(frag_color_module, None);
        }
        
        Ok((pipelines[0], pipelines[1]))
    }
    
    fn create_shader_module(device: &ash::Device, code: &[u8]) -> Result<vk::ShaderModule, Box<dyn std::error::Error>> {
        let code_u32: Vec<u32> = code.chunks_exact(4)
            .map(|chunk| u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
            .collect();
        
        let create_info = vk::ShaderModuleCreateInfo::default()
            .code(&code_u32);
        
        Ok(unsafe { device.create_shader_module(&create_info, None)? })
    }
    
    fn create_render_targets(
        device: &ash::Device,
        instance: &ash::Instance,
        physical_device: vk::PhysicalDevice,
        gbm: &GbmDevice<std::fs::File>,
        render_pass: vk::RenderPass,
        width: u32,
        height: u32,
        card: &DrmCard,
    ) -> Result<Vec<RenderTarget>, Box<dyn std::error::Error>> {
        let mut targets = Vec::new();
        let mem_props = unsafe { instance.get_physical_device_memory_properties(physical_device) };
        
        for i in 0..2 {
            let bo = unsafe {
                gbm_sys::gbm_bo_create(
                    gbm.as_raw() as *mut _,
                    width,
                    height,
                    0x34325258,
                    1 | 8 | 16,
                )
            };
            
            if bo.is_null() {
                return Err("Failed to create GBM buffer".into());
            }
            
            let gbm_fd = unsafe { gbm_sys::gbm_bo_get_fd(bo) };
            let stride = unsafe { gbm_sys::gbm_bo_get_stride(bo) };
            log::info!("[vulkan] Created GBM buffer {}: fd={} stride={}", i, gbm_fd, stride);
            
            let image_info = vk::ImageCreateInfo::default()
                .image_type(vk::ImageType::TYPE_2D)
                .format(vk::Format::B8G8R8A8_UNORM)
                .extent(vk::Extent3D { width, height, depth: 1 })
                .mip_levels(1)
                .array_layers(1)
                .samples(vk::SampleCountFlags::TYPE_1)
                .tiling(vk::ImageTiling::OPTIMAL)
                .usage(vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::TRANSFER_SRC)
                .sharing_mode(vk::SharingMode::EXCLUSIVE);
            
            let image = unsafe { device.create_image(&image_info, None)? };
            
            let mem_requirements = unsafe { device.get_image_memory_requirements(image) };
            
            let memory_type_index = (0..mem_props.memory_type_count)
                .find(|&i| {
                    let type_bits = 1 << i;
                    let props = mem_props.memory_types[i as usize].property_flags;
                    (mem_requirements.memory_type_bits & type_bits) != 0 &&
                        props.contains(vk::MemoryPropertyFlags::DEVICE_LOCAL)
                })
                .ok_or("No suitable memory type for render target")?;
            
            let alloc_info = vk::MemoryAllocateInfo::default()
                .allocation_size(mem_requirements.size)
                .memory_type_index(memory_type_index);
            
            let memory = unsafe { device.allocate_memory(&alloc_info, None)? };
            unsafe { device.bind_image_memory(image, memory, 0)? };
            
            let buffer_size = (width * height * 4) as vk::DeviceSize;
            let staging_buffer_info = vk::BufferCreateInfo::default()
                .size(buffer_size)
                .usage(vk::BufferUsageFlags::TRANSFER_DST)
                .sharing_mode(vk::SharingMode::EXCLUSIVE);
            
            let staging_buffer = unsafe { device.create_buffer(&staging_buffer_info, None)? };
            let staging_mem_req = unsafe { device.get_buffer_memory_requirements(staging_buffer) };
            
            let staging_memory_type = (0..mem_props.memory_type_count)
                .find(|&i| {
                    let type_bits = 1 << i;
                    let props = mem_props.memory_types[i as usize].property_flags;
                    (staging_mem_req.memory_type_bits & type_bits) != 0 &&
                        props.contains(vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT)
                })
                .ok_or("No suitable memory type for staging buffer")?;
            
            let staging_alloc_info = vk::MemoryAllocateInfo::default()
                .allocation_size(staging_mem_req.size)
                .memory_type_index(staging_memory_type);
            
            let staging_memory = unsafe { device.allocate_memory(&staging_alloc_info, None)? };
            unsafe { device.bind_buffer_memory(staging_buffer, staging_memory, 0)? };
            
            let staging_ptr = unsafe {
                device.map_memory(staging_memory, 0, buffer_size, vk::MemoryMapFlags::empty())?
            };
            
            let view_info = vk::ImageViewCreateInfo::default()
                .image(image)
                .view_type(vk::ImageViewType::TYPE_2D)
                .format(vk::Format::B8G8R8A8_UNORM)
                .subresource_range(vk::ImageSubresourceRange {
                    aspect_mask: vk::ImageAspectFlags::COLOR,
                    base_mip_level: 0,
                    level_count: 1,
                    base_array_layer: 0,
                    layer_count: 1,
                });
            
            let view = unsafe { device.create_image_view(&view_info, None)? };
            
            let fb_info = vk::FramebufferCreateInfo::default()
                .render_pass(render_pass)
                .attachments(std::slice::from_ref(&view))
                .width(width)
                .height(height)
                .layers(1);
            
            let framebuffer = unsafe { device.create_framebuffer(&fb_info, None)? };
            
            let handle = unsafe { gbm_sys::gbm_bo_get_handle(bo).u32_ };
            
            struct GbmBuffer {
                handle: u32,
                width: u32,
                height: u32,
                stride: u32,
            }
            
            impl drm::buffer::Buffer for GbmBuffer {
                fn size(&self) -> (u32, u32) { (self.width, self.height) }
                fn format(&self) -> drm::buffer::DrmFourcc { drm::buffer::DrmFourcc::Xrgb8888 }
                fn pitch(&self) -> u32 { self.stride }
                fn handle(&self) -> drm::buffer::Handle {
                    drm::buffer::Handle::from(std::num::NonZeroU32::new(self.handle).unwrap())
                }
            }
            
            let buffer = GbmBuffer { handle, width, height, stride };
            let drm_fb = card.add_framebuffer(&buffer, 24, 32).ok();
            
            if drm_fb.is_some() {
                log::info!("[vulkan] Created DRM framebuffer {:?} for render target {}", drm_fb.unwrap(), i);
            }
            
            let mut gbm_map_data: *mut std::ffi::c_void = std::ptr::null_mut();
            let mut gbm_stride_out = stride;
            let gbm_ptr = unsafe {
                gbm_sys::gbm_bo_map(
                    bo,
                    0, 0,
                    width, height,
                    gbm_sys::gbm_bo_transfer_flags::GBM_BO_TRANSFER_WRITE,
                    &mut gbm_stride_out,
                    &mut gbm_map_data,
                )
            };
            
            if gbm_ptr.is_null() {
                log::warn!("[vulkan] Failed to map GBM buffer {} - will use alternative path", i);
            } else {
                log::info!("[vulkan] Mapped GBM buffer {}: stride={}", i, gbm_stride_out);
            }
            
            targets.push(RenderTarget {
                gbm_bo: bo,
                gbm_ptr: gbm_ptr as *mut u8,
                gbm_stride: gbm_stride_out,
                gbm_map_data,
                image,
                memory,
                view,
                framebuffer,
                drm_fb,
                staging_buffer,
                staging_memory,
                staging_ptr: staging_ptr as *mut u8,
            });
        }
        
        Ok(targets)
    }
    
    fn query_dmabuf_formats() -> Vec<DmaBufFormat> {
        vec![
            DmaBufFormat { format: drm_fourcc::DrmFourcc::Argb8888 as u32, modifier: drm_fourcc::DrmModifier::Linear.into() },
            DmaBufFormat { format: drm_fourcc::DrmFourcc::Xrgb8888 as u32, modifier: drm_fourcc::DrmModifier::Linear.into() },
            DmaBufFormat { format: drm_fourcc::DrmFourcc::Abgr8888 as u32, modifier: drm_fourcc::DrmModifier::Linear.into() },
            DmaBufFormat { format: drm_fourcc::DrmFourcc::Xbgr8888 as u32, modifier: drm_fourcc::DrmModifier::Linear.into() },
        ]
    }
    
    pub fn begin_frame(&mut self) {
        let target = &self.render_images[self.current_target];
        
        let begin_info = vk::CommandBufferBeginInfo::default()
            .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
        
        unsafe {
            self.device.reset_command_buffer(self.command_buffer, vk::CommandBufferResetFlags::empty()).ok();
            self.device.begin_command_buffer(self.command_buffer, &begin_info).ok();
            
            let clear_value = vk::ClearValue {
                color: vk::ClearColorValue {
                    float32: [0.1, 0.1, 0.12, 1.0],
                },
            };
            
            let render_pass_info = vk::RenderPassBeginInfo::default()
                .render_pass(self.render_pass)
                .framebuffer(target.framebuffer)
                .render_area(vk::Rect2D {
                    offset: vk::Offset2D { x: 0, y: 0 },
                    extent: vk::Extent2D { width: self.width, height: self.height },
                })
                .clear_values(std::slice::from_ref(&clear_value));
            
            self.device.cmd_begin_render_pass(self.command_buffer, &render_pass_info, vk::SubpassContents::INLINE);
        }
    }
    
    pub fn end_frame(&mut self) {
        let target = &self.render_images[self.current_target];
        
        unsafe {
            self.device.cmd_end_render_pass(self.command_buffer);
            
            let barrier = vk::ImageMemoryBarrier::default()
                .old_layout(vk::ImageLayout::GENERAL)
                .new_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .image(target.image)
                .subresource_range(vk::ImageSubresourceRange {
                    aspect_mask: vk::ImageAspectFlags::COLOR,
                    base_mip_level: 0,
                    level_count: 1,
                    base_array_layer: 0,
                    layer_count: 1,
                })
                .src_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE)
                .dst_access_mask(vk::AccessFlags::TRANSFER_READ);
            
            self.device.cmd_pipeline_barrier(
                self.command_buffer,
                vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
                vk::PipelineStageFlags::TRANSFER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[barrier],
            );
            
            let region = vk::BufferImageCopy::default()
                .buffer_offset(0)
                .buffer_row_length(self.width)
                .buffer_image_height(self.height)
                .image_subresource(vk::ImageSubresourceLayers {
                    aspect_mask: vk::ImageAspectFlags::COLOR,
                    mip_level: 0,
                    base_array_layer: 0,
                    layer_count: 1,
                })
                .image_offset(vk::Offset3D { x: 0, y: 0, z: 0 })
                .image_extent(vk::Extent3D { width: self.width, height: self.height, depth: 1 });
            
            self.device.cmd_copy_image_to_buffer(
                self.command_buffer,
                target.image,
                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                target.staging_buffer,
                &[region],
            );
            
            let buffer_barrier = vk::BufferMemoryBarrier::default()
                .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                .dst_access_mask(vk::AccessFlags::HOST_READ)
                .buffer(target.staging_buffer)
                .offset(0)
                .size(vk::WHOLE_SIZE);
            
            self.device.cmd_pipeline_barrier(
                self.command_buffer,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::HOST,
                vk::DependencyFlags::empty(),
                &[],
                &[buffer_barrier],
                &[],
            );
            
            self.device.end_command_buffer(self.command_buffer).ok();
            
            let submit_info = vk::SubmitInfo::default()
                .command_buffers(std::slice::from_ref(&self.command_buffer));
            
            self.device.queue_submit(self.queue, &[submit_info], vk::Fence::null()).ok();
            self.device.queue_wait_idle(self.queue).ok();
        }
        
        if !target.gbm_ptr.is_null() && !target.staging_ptr.is_null() {
            let src_stride = self.width * 4;
            let dst_stride = target.gbm_stride;
            
            unsafe {
                if src_stride == dst_stride {
                    std::ptr::copy_nonoverlapping(
                        target.staging_ptr,
                        target.gbm_ptr,
                        (self.height * src_stride) as usize,
                    );
                } else {
                    for y in 0..self.height {
                        let src_offset = (y * src_stride) as isize;
                        let dst_offset = (y * dst_stride) as isize;
                        std::ptr::copy_nonoverlapping(
                            target.staging_ptr.offset(src_offset),
                            target.gbm_ptr.offset(dst_offset),
                            (self.width * 4) as usize,
                        );
                    }
                }
            }
        }
        
        if let Some(drm_fb) = target.drm_fb {
            if !self.mode_set {
                if let Err(e) = self.drm_card.set_crtc(
                    self.crtc,
                    Some(drm_fb),
                    (0, 0),
                    &[self.connector],
                    Some(self.mode),
                ) {
                    log::error!("[vulkan] set_crtc failed: {}", e);
                } else {
                    self.mode_set = true;
                }
            } else {
                use drm::control::PageFlipFlags;
                if let Err(_e) = self.drm_card.page_flip(self.crtc, drm_fb, PageFlipFlags::EVENT, None) {
                    self.drm_card.set_crtc(
                        self.crtc,
                        Some(drm_fb),
                        (0, 0),
                        &[self.connector],
                        Some(self.mode),
                    ).ok();
                }
            }
        }
        
        self.current_target = (self.current_target + 1) % self.render_images.len();
    }
    
    pub fn draw_rect(&mut self, x: i32, y: i32, width: i32, height: i32, color: [f32; 4]) {
        let push_constants = PushConstants {
            offset: [x as f32, y as f32],
            size: [width as f32, height as f32],
            screen_size: [self.width as f32, self.height as f32],
            _padding: [0.0, 0.0],
            color,
        };
        
        unsafe {
            self.device.cmd_bind_pipeline(self.command_buffer, vk::PipelineBindPoint::GRAPHICS, self.color_pipeline);
            self.device.cmd_push_constants(
                self.command_buffer,
                self.pipeline_layout,
                vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
                0,
                std::slice::from_raw_parts(
                    &push_constants as *const PushConstants as *const u8,
                    std::mem::size_of::<PushConstants>(),
                ),
            );
            self.device.cmd_draw(self.command_buffer, 6, 1, 0, 0);
        }
    }
    
    pub fn upload_shm_texture(&mut self, id: u64, width: u32, height: u32, stride: u32, data: &[u8]) -> u64 {
        if let Some(old) = self.shm_textures.remove(&id) {
            self.destroy_texture(old);
        }
        
        if let Some(texture) = self.create_texture_from_data(width, height, stride, data) {
            self.shm_textures.insert(id, texture);
        }
        id
    }
    
    fn create_texture_from_data(&mut self, width: u32, height: u32, _stride: u32, data: &[u8]) -> Option<VulkanTexture> {
        let buffer_size = (width * height * 4) as vk::DeviceSize;
        
        let buffer_info = vk::BufferCreateInfo::default()
            .size(buffer_size)
            .usage(vk::BufferUsageFlags::TRANSFER_SRC)
            .sharing_mode(vk::SharingMode::EXCLUSIVE);
        
        let staging_buffer = unsafe { self.device.create_buffer(&buffer_info, None).ok()? };
        
        let mem_requirements = unsafe { self.device.get_buffer_memory_requirements(staging_buffer) };
        let mem_props = unsafe { self.instance.get_physical_device_memory_properties(self.physical_device) };
        
        let staging_memory_type = (0..mem_props.memory_type_count)
            .find(|&i| {
                let type_bits = 1 << i;
                let props = mem_props.memory_types[i as usize].property_flags;
                (mem_requirements.memory_type_bits & type_bits) != 0 &&
                    props.contains(vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT)
            })?;
        
        let alloc_info = vk::MemoryAllocateInfo::default()
            .allocation_size(mem_requirements.size)
            .memory_type_index(staging_memory_type);
        
        let staging_memory = unsafe { self.device.allocate_memory(&alloc_info, None).ok()? };
        unsafe { self.device.bind_buffer_memory(staging_buffer, staging_memory, 0).ok()? };
        
        unsafe {
            let ptr = self.device.map_memory(staging_memory, 0, buffer_size, vk::MemoryMapFlags::empty()).ok()?;
            std::ptr::copy_nonoverlapping(data.as_ptr(), ptr as *mut u8, data.len().min(buffer_size as usize));
            self.device.unmap_memory(staging_memory);
        }
        
        let image_info = vk::ImageCreateInfo::default()
            .image_type(vk::ImageType::TYPE_2D)
            .format(vk::Format::B8G8R8A8_UNORM)
            .extent(vk::Extent3D { width, height, depth: 1 })
            .mip_levels(1)
            .array_layers(1)
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(vk::ImageTiling::OPTIMAL)
            .usage(vk::ImageUsageFlags::TRANSFER_DST | vk::ImageUsageFlags::SAMPLED)
            .sharing_mode(vk::SharingMode::EXCLUSIVE);
        
        let image = unsafe { self.device.create_image(&image_info, None).ok()? };
        
        let img_mem_requirements = unsafe { self.device.get_image_memory_requirements(image) };
        
        let image_memory_type = (0..mem_props.memory_type_count)
            .find(|&i| {
                let type_bits = 1 << i;
                let props = mem_props.memory_types[i as usize].property_flags;
                (img_mem_requirements.memory_type_bits & type_bits) != 0 &&
                    props.contains(vk::MemoryPropertyFlags::DEVICE_LOCAL)
            })?;
        
        let alloc_info = vk::MemoryAllocateInfo::default()
            .allocation_size(img_mem_requirements.size)
            .memory_type_index(image_memory_type);
        
        let memory = unsafe { self.device.allocate_memory(&alloc_info, None).ok()? };
        unsafe { self.device.bind_image_memory(image, memory, 0).ok()? };
        
        let cmd_alloc_info = vk::CommandBufferAllocateInfo::default()
            .command_pool(self.command_pool)
            .level(vk::CommandBufferLevel::PRIMARY)
            .command_buffer_count(1);
        
        let copy_cmd = unsafe { self.device.allocate_command_buffers(&cmd_alloc_info).ok()?[0] };
        
        let begin_info = vk::CommandBufferBeginInfo::default()
            .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
        
        unsafe {
            self.device.begin_command_buffer(copy_cmd, &begin_info).ok()?;
            
            let barrier = vk::ImageMemoryBarrier::default()
                .old_layout(vk::ImageLayout::UNDEFINED)
                .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .image(image)
                .subresource_range(vk::ImageSubresourceRange {
                    aspect_mask: vk::ImageAspectFlags::COLOR,
                    base_mip_level: 0,
                    level_count: 1,
                    base_array_layer: 0,
                    layer_count: 1,
                })
                .src_access_mask(vk::AccessFlags::empty())
                .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE);
            
            self.device.cmd_pipeline_barrier(
                copy_cmd,
                vk::PipelineStageFlags::TOP_OF_PIPE,
                vk::PipelineStageFlags::TRANSFER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[barrier],
            );
            
            let region = vk::BufferImageCopy::default()
                .buffer_offset(0)
                .buffer_row_length(0)
                .buffer_image_height(0)
                .image_subresource(vk::ImageSubresourceLayers {
                    aspect_mask: vk::ImageAspectFlags::COLOR,
                    mip_level: 0,
                    base_array_layer: 0,
                    layer_count: 1,
                })
                .image_offset(vk::Offset3D { x: 0, y: 0, z: 0 })
                .image_extent(vk::Extent3D { width, height, depth: 1 });
            
            self.device.cmd_copy_buffer_to_image(
                copy_cmd,
                staging_buffer,
                image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &[region],
            );
            
            let barrier = vk::ImageMemoryBarrier::default()
                .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .image(image)
                .subresource_range(vk::ImageSubresourceRange {
                    aspect_mask: vk::ImageAspectFlags::COLOR,
                    base_mip_level: 0,
                    level_count: 1,
                    base_array_layer: 0,
                    layer_count: 1,
                })
                .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                .dst_access_mask(vk::AccessFlags::SHADER_READ);
            
            self.device.cmd_pipeline_barrier(
                copy_cmd,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::FRAGMENT_SHADER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[barrier],
            );
            
            self.device.end_command_buffer(copy_cmd).ok()?;
            
            let submit_info = vk::SubmitInfo::default()
                .command_buffers(std::slice::from_ref(&copy_cmd));
            
            self.device.queue_submit(self.queue, &[submit_info], vk::Fence::null()).ok()?;
            self.device.queue_wait_idle(self.queue).ok()?;
            
            self.device.free_command_buffers(self.command_pool, &[copy_cmd]);
            self.device.destroy_buffer(staging_buffer, None);
            self.device.free_memory(staging_memory, None);
        }
        
        let view_info = vk::ImageViewCreateInfo::default()
            .image(image)
            .view_type(vk::ImageViewType::TYPE_2D)
            .format(vk::Format::B8G8R8A8_UNORM)
            .subresource_range(vk::ImageSubresourceRange {
                aspect_mask: vk::ImageAspectFlags::COLOR,
                base_mip_level: 0,
                level_count: 1,
                base_array_layer: 0,
                layer_count: 1,
            });
        
        let view = unsafe { self.device.create_image_view(&view_info, None).ok()? };
        
        let ds_alloc_info = vk::DescriptorSetAllocateInfo::default()
            .descriptor_pool(self.descriptor_pool)
            .set_layouts(std::slice::from_ref(&self.descriptor_set_layout));
        
        let descriptor_set = unsafe { self.device.allocate_descriptor_sets(&ds_alloc_info).ok()?[0] };
        
        let desc_image_info = vk::DescriptorImageInfo::default()
            .sampler(self.sampler)
            .image_view(view)
            .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL);
        
        let write = vk::WriteDescriptorSet::default()
            .dst_set(descriptor_set)
            .dst_binding(0)
            .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
            .image_info(std::slice::from_ref(&desc_image_info));
        
        unsafe { self.device.update_descriptor_sets(&[write], &[]) };
        
        Some(VulkanTexture {
            image,
            memory,
            view,
            descriptor_set,
            width,
            height,
        })
    }
    
    pub fn import_dmabuf_texture(
        &mut self,
        id: u64,
        fd: RawFd,
        width: u32,
        height: u32,
        format: u32,
        stride: u32,
        offset: u32,
        modifier: u64,
    ) -> Option<()> {
        if let Some(old) = self.dmabuf_textures.remove(&id) {
            self.destroy_texture(old);
        }
        
        log::debug!("[vulkan] Importing DMA-BUF: id={} fd={} {}x{} format={:#x} stride={} offset={} modifier={:#x}",
            id, fd, width, height, format, stride, offset, modifier);
        
        let vk_format = match format {
            f if f == drm_fourcc::DrmFourcc::Argb8888 as u32 => vk::Format::B8G8R8A8_UNORM,
            f if f == drm_fourcc::DrmFourcc::Xrgb8888 as u32 => vk::Format::B8G8R8A8_UNORM,
            f if f == drm_fourcc::DrmFourcc::Abgr8888 as u32 => vk::Format::R8G8B8A8_UNORM,
            f if f == drm_fourcc::DrmFourcc::Xbgr8888 as u32 => vk::Format::R8G8B8A8_UNORM,
            _ => {
                log::warn!("[vulkan] Unsupported DRM format: {:#x}", format);
                return None;
            }
        };
        
        let mut external_info = vk::ExternalMemoryImageCreateInfo::default()
            .handle_types(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT);
        
        let image_info = vk::ImageCreateInfo::default()
            .push_next(&mut external_info)
            .image_type(vk::ImageType::TYPE_2D)
            .format(vk_format)
            .extent(vk::Extent3D { width, height, depth: 1 })
            .mip_levels(1)
            .array_layers(1)
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(vk::ImageTiling::LINEAR)
            .usage(vk::ImageUsageFlags::SAMPLED)
            .sharing_mode(vk::SharingMode::EXCLUSIVE);
        
        let image = unsafe { self.device.create_image(&image_info, None).ok()? };
        
        let mem_requirements = unsafe { self.device.get_image_memory_requirements(image) };
        let mem_props = unsafe { self.instance.get_physical_device_memory_properties(self.physical_device) };
        
        let memory_type_index = (0..mem_props.memory_type_count)
            .find(|&i| {
                let type_bits = 1 << i;
                (mem_requirements.memory_type_bits & type_bits) != 0
            })
            .unwrap_or(0);
        
        let dup_fd = unsafe { libc::dup(fd) };
        if dup_fd < 0 {
            log::warn!("[vulkan] Failed to dup fd");
            unsafe { self.device.destroy_image(image, None) };
            return None;
        }
        
        let mut import_info = vk::ImportMemoryFdInfoKHR::default()
            .handle_type(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT)
            .fd(dup_fd);
        
        let alloc_info = vk::MemoryAllocateInfo::default()
            .push_next(&mut import_info)
            .allocation_size(mem_requirements.size)
            .memory_type_index(memory_type_index);
        
        let memory = match unsafe { self.device.allocate_memory(&alloc_info, None) } {
            Ok(m) => m,
            Err(e) => {
                log::warn!("[vulkan] Failed to allocate memory for DMA-BUF: {:?}", e);
                unsafe {
                    libc::close(dup_fd);
                    self.device.destroy_image(image, None);
                }
                return None;
            }
        };
        
        if let Err(e) = unsafe { self.device.bind_image_memory(image, memory, 0) } {
            log::warn!("[vulkan] Failed to bind DMA-BUF memory: {:?}", e);
            unsafe {
                self.device.free_memory(memory, None);
                self.device.destroy_image(image, None);
            }
            return None;
        }
        
        let view_info = vk::ImageViewCreateInfo::default()
            .image(image)
            .view_type(vk::ImageViewType::TYPE_2D)
            .format(vk_format)
            .subresource_range(vk::ImageSubresourceRange {
                aspect_mask: vk::ImageAspectFlags::COLOR,
                base_mip_level: 0,
                level_count: 1,
                base_array_layer: 0,
                layer_count: 1,
            });
        
        let view = match unsafe { self.device.create_image_view(&view_info, None) } {
            Ok(v) => v,
            Err(e) => {
                log::warn!("[vulkan] Failed to create image view: {:?}", e);
                unsafe {
                    self.device.free_memory(memory, None);
                    self.device.destroy_image(image, None);
                }
                return None;
            }
        };
        
        let alloc_info = vk::DescriptorSetAllocateInfo::default()
            .descriptor_pool(self.descriptor_pool)
            .set_layouts(std::slice::from_ref(&self.descriptor_set_layout));
        
        let descriptor_set = match unsafe { self.device.allocate_descriptor_sets(&alloc_info) } {
            Ok(sets) => sets[0],
            Err(e) => {
                log::warn!("[vulkan] Failed to allocate descriptor set: {:?}", e);
                unsafe {
                    self.device.destroy_image_view(view, None);
                    self.device.free_memory(memory, None);
                    self.device.destroy_image(image, None);
                }
                return None;
            }
        };
        
        let image_info = vk::DescriptorImageInfo::default()
            .sampler(self.sampler)
            .image_view(view)
            .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL);
        
        let write = vk::WriteDescriptorSet::default()
            .dst_set(descriptor_set)
            .dst_binding(0)
            .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
            .image_info(std::slice::from_ref(&image_info));
        
        unsafe { self.device.update_descriptor_sets(&[write], &[]) };
        
        log::debug!("[vulkan] Successfully imported DMA-BUF texture id={}", id);
        
        self.dmabuf_textures.insert(id, VulkanTexture {
            image,
            memory,
            view,
            descriptor_set,
            width,
            height,
        });
        
        Some(())
    }
    
    pub fn draw_texture(&mut self, id: u64, x: i32, y: i32, width: i32, height: i32) {
        let texture = match self.shm_textures.get(&id).or_else(|| self.dmabuf_textures.get(&id)) {
            Some(t) => t,
            None => return,
        };
        
        let push_constants = PushConstants {
            offset: [x as f32, y as f32],
            size: [width as f32, height as f32],
            screen_size: [self.width as f32, self.height as f32],
            _padding: [0.0, 0.0],
            color: [1.0, 1.0, 1.0, 1.0],
        };
        
        unsafe {
            self.device.cmd_bind_pipeline(self.command_buffer, vk::PipelineBindPoint::GRAPHICS, self.texture_pipeline);
            self.device.cmd_bind_descriptor_sets(
                self.command_buffer,
                vk::PipelineBindPoint::GRAPHICS,
                self.pipeline_layout,
                0,
                &[texture.descriptor_set],
                &[],
            );
            self.device.cmd_push_constants(
                self.command_buffer,
                self.pipeline_layout,
                vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
                0,
                std::slice::from_raw_parts(
                    &push_constants as *const PushConstants as *const u8,
                    std::mem::size_of::<PushConstants>(),
                ),
            );
            self.device.cmd_draw(self.command_buffer, 6, 1, 0, 0);
        }
    }
    
    pub fn draw_dmabuf_texture(&mut self, id: u64, x: i32, y: i32, width: i32, height: i32) {
        self.draw_texture(id, x, y, width, height);
    }
    
    fn destroy_texture(&mut self, texture: VulkanTexture) {
        unsafe {
            self.device.free_descriptor_sets(self.descriptor_pool, &[texture.descriptor_set]).ok();
            self.device.destroy_image_view(texture.view, None);
            self.device.free_memory(texture.memory, None);
            self.device.destroy_image(texture.image, None);
        }
    }
    
    pub fn remove_texture(&mut self, id: u64) {
        if let Some(tex) = self.shm_textures.remove(&id) {
            self.destroy_texture(tex);
        }
        if let Some(tex) = self.dmabuf_textures.remove(&id) {
            self.destroy_texture(tex);
        }
    }
    
    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }
    
    pub fn physical_size(&self) -> (u32, u32) {
        (self.physical_width, self.physical_height)
    }
    
    pub fn drm_fd(&self) -> BorrowedFd<'_> {
        self.drm_card.as_fd()
    }
    
    pub fn drm_dev(&self) -> u64 {
        let fd = self.drm_card.as_fd().as_raw_fd();
        unsafe {
            let mut stat: libc::stat = std::mem::zeroed();
            if libc::fstat(fd, &mut stat) == 0 {
                stat.st_rdev
            } else {
                0
            }
        }
    }
    
    pub fn render_node_dev(&self) -> u64 {
        let card_dev = self.drm_dev();
        if card_dev == 0 { return 0; }
        
        let card_minor = (card_dev & 0xff) as u32;
        let render_minor = 128 + card_minor;
        let render_path = format!("/dev/dri/renderD{}", render_minor);
        
        if let Ok(meta) = std::fs::metadata(&render_path) {
            use std::os::unix::fs::MetadataExt;
            return meta.rdev();
        }
        
        card_dev
    }
    
    pub fn is_flip_pending(&self) -> bool {
        false
    }
    
    pub fn handle_drm_event(&mut self) -> bool {
        false
    }
    
    pub fn texture_count(&self) -> usize {
        self.shm_textures.len() + self.dmabuf_textures.len()
    }
    
    pub fn draw_profiler(&mut self, stats: &crate::renderer::ProfilerStats) {
        const FONT_DATA: &[u8] = include_bytes!("font5x7.raw");
        const FONT_CHAR_WIDTH: usize = 5;
        const FONT_CHAR_HEIGHT: usize = 7;
        const FONT_CHARS_PER_ROW: usize = 16;
        const PROFILER_TEXTURE_ID: u64 = u64::MAX - 1;
        
        let lines = [
            format!("FPS: {:.1}", stats.fps),
            format!("Frame: {:.2}ms", stats.frame_time_ms),
            format!("Render: {}us", stats.render_time_us),
            format!("Input: {}us", stats.input_time_us),
            format!("Mem: {:.1}MB", stats.memory_mb),
            format!("Windows: {}", stats.window_count),
            format!("Textures: {}", stats.texture_count),
        ];
        
        let scale: usize = 2;
        let char_w = FONT_CHAR_WIDTH * scale;
        let char_h = FONT_CHAR_HEIGHT * scale;
        let line_height = char_h + 2;
        let padding: usize = 8;
        
        let max_chars = lines.iter().map(|l| l.len()).max().unwrap_or(0);
        let box_width = max_chars * char_w + padding * 2;
        let box_height = lines.len() * line_height + padding * 2;
        let mut pixels = vec![0u8; box_width * box_height * 4];
        
        for i in 0..(box_width * box_height) {
            pixels[i * 4] = 0;
            pixels[i * 4 + 1] = 0;
            pixels[i * 4 + 2] = 0;
            pixels[i * 4 + 3] = 180;
        }
        
        for (line_idx, line) in lines.iter().enumerate() {
            let text_y = padding + line_idx * line_height;
            for (char_idx, ch) in line.chars().enumerate() {
                let text_x = padding + char_idx * char_w;
                Self::draw_char_to_buffer(&mut pixels, box_width, text_x, text_y, ch, scale, FONT_DATA, FONT_CHAR_WIDTH, FONT_CHAR_HEIGHT, FONT_CHARS_PER_ROW);
            }
        }
        
        self.upload_shm_texture(PROFILER_TEXTURE_ID, box_width as u32, box_height as u32, (box_width * 4) as u32, &pixels);
        
        let box_x = self.width as i32 - box_width as i32 - 10;
        let box_y = 10;
        
        self.draw_texture(PROFILER_TEXTURE_ID, box_x, box_y, box_width as i32, box_height as i32);
    }
    
    fn draw_char_to_buffer(
        pixels: &mut [u8],
        stride: usize,
        x: usize,
        y: usize,
        ch: char,
        scale: usize,
        font_data: &[u8],
        font_char_width: usize,
        font_char_height: usize,
        font_chars_per_row: usize,
    ) {
        let idx = if ch.is_ascii() && ch >= ' ' {
            (ch as usize) - 32
        } else {
            0
        };
        
        let font_x = (idx % font_chars_per_row) * font_char_width;
        let font_y = (idx / font_chars_per_row) * font_char_height;
        
        for cy in 0..font_char_height {
            for cx in 0..font_char_width {
                let px = font_x + cx;
                let py = font_y + cy;
                let byte_idx = py * (font_chars_per_row * font_char_width) + px;
                
                if byte_idx < font_data.len() && font_data[byte_idx] > 128 {
                    for sy in 0..scale {
                        for sx in 0..scale {
                            let dest_x = x + cx * scale + sx;
                            let dest_y = y + cy * scale + sy;
                            if dest_x < stride && dest_y * stride + dest_x < pixels.len() / 4 {
                                let dest_idx = (dest_y * stride + dest_x) * 4;
                                if dest_idx + 3 < pixels.len() {
                                    pixels[dest_idx] = 255;
                                    pixels[dest_idx + 1] = 255;
                                    pixels[dest_idx + 2] = 255;
                                    pixels[dest_idx + 3] = 255;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

impl Drop for VulkanRenderer {
    fn drop(&mut self) {
        unsafe {
            self.device.device_wait_idle().ok();
            
            for (_, tex) in self.shm_textures.drain() {
                self.device.destroy_image_view(tex.view, None);
                self.device.free_memory(tex.memory, None);
                self.device.destroy_image(tex.image, None);
            }
            
            for (_, tex) in self.dmabuf_textures.drain() {
                self.device.destroy_image_view(tex.view, None);
                self.device.free_memory(tex.memory, None);
                self.device.destroy_image(tex.image, None);
            }
            
            for target in &self.render_images {
                if let Some(fb) = target.drm_fb {
                    self.drm_card.destroy_framebuffer(fb).ok();
                }
                self.device.destroy_framebuffer(target.framebuffer, None);
                self.device.destroy_image_view(target.view, None);
                self.device.free_memory(target.memory, None);
                self.device.destroy_image(target.image, None);
                self.device.unmap_memory(target.staging_memory);
                self.device.destroy_buffer(target.staging_buffer, None);
                self.device.free_memory(target.staging_memory, None);
                
                if !target.gbm_bo.is_null() {
                    if !target.gbm_map_data.is_null() {
                        gbm_sys::gbm_bo_unmap(target.gbm_bo, target.gbm_map_data);
                    }
                    gbm_sys::gbm_bo_destroy(target.gbm_bo);
                }
            }
            
            self.device.destroy_descriptor_pool(self.descriptor_pool, None);
            self.device.destroy_sampler(self.sampler, None);
            self.device.destroy_pipeline(self.texture_pipeline, None);
            self.device.destroy_pipeline(self.color_pipeline, None);
            self.device.destroy_pipeline_layout(self.pipeline_layout, None);
            self.device.destroy_descriptor_set_layout(self.descriptor_set_layout, None);
            self.device.destroy_render_pass(self.render_pass, None);
            self.device.destroy_command_pool(self.command_pool, None);
            self.device.destroy_device(None);
            self.instance.destroy_instance(None);
        }
    }
}
