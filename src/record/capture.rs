use glow::HasContext;

/// Double-buffered PBO readback for async GPU→CPU pixel transfer.
/// Uses two PBOs: while one is being filled by glReadPixels (DMA), the other
/// is mapped to CPU memory to build the frame. Two frames of pipeline delay
/// prevents GPU stalls.
pub struct PboReader {
    pbos: [glow::Buffer; 2],
    fbo: glow::Framebuffer,
    write_index: usize,
    read_index: usize,
    width: u32,
    height: u32,
    /// Number of frames submitted. Readback only starts after frame 1.
    frame_count: u64,
}

impl PboReader {
    pub fn new(gl: &glow::Context, width: u32, height: u32) -> Self {
        let size = (width * height * 4) as i32;
        unsafe {
            let mut pbos = [gl.create_buffer().unwrap(), gl.create_buffer().unwrap()];
            for pbo in &pbos {
                gl.bind_buffer(glow::PIXEL_PACK_BUFFER, Some(*pbo));
                gl.buffer_data_size(
                    glow::PIXEL_PACK_BUFFER,
                    size,
                    glow::STREAM_READ,
                );
            }
            gl.bind_buffer(glow::PIXEL_PACK_BUFFER, None);

            let fbo = gl.create_framebuffer().unwrap();

            Self {
                pbos,
                fbo,
                write_index: 0,
                read_index: 1,
                width,
                height,
                frame_count: 0,
            }
        }
    }

    /// Call after rendering: triggers glReadPixels into the write PBO.
    pub fn begin_frame(&mut self, gl: &glow::Context, src_tex: glow::Texture) {
        unsafe {
            gl.bind_framebuffer(glow::READ_FRAMEBUFFER, Some(self.fbo));
            gl.framebuffer_texture_2d(
                glow::READ_FRAMEBUFFER,
                glow::COLOR_ATTACHMENT0,
                glow::TEXTURE_2D,
                Some(src_tex),
                0,
            );
            gl.bind_buffer(glow::PIXEL_PACK_BUFFER, Some(self.pbos[self.write_index]));
            gl.read_pixels(
                0,
                0,
                self.width as i32,
                self.height as i32,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(None),
            );
            gl.bind_buffer(glow::PIXEL_PACK_BUFFER, None);
            gl.bind_framebuffer(glow::READ_FRAMEBUFFER, None);
        }
        self.frame_count += 1;
    }

    /// Call at end of frame: reads back the oldest completed PBO.
    /// Returns None if we don't have enough frames in flight yet.
    pub fn end_frame(&mut self, gl: &glow::Context) -> Option<Vec<u8>> {
        if self.frame_count < 2 {
            // Swap indices anyway so pipeline advances
            std::mem::swap(&mut self.write_index, &mut self.read_index);
            return None;
        }

        let data = unsafe {
            gl.bind_buffer(glow::PIXEL_PACK_BUFFER, Some(self.pbos[self.read_index]));
            let ptr = gl.map_buffer_range(
                glow::PIXEL_PACK_BUFFER,
                0,
                (self.width * self.height * 4) as i32,
                glow::MAP_READ_BIT,
            );
            let data = if ptr.is_null() {
                log::warn!("PBO map returned null");
                Vec::new()
            } else {
                std::slice::from_raw_parts(ptr as *const u8, (self.width * self.height * 4) as usize).to_vec()
            };
            gl.unmap_buffer(glow::PIXEL_PACK_BUFFER);
            gl.bind_buffer(glow::PIXEL_PACK_BUFFER, None);
            data
        };

        self.read_index = self.write_index;
        self.write_index = (self.write_index + 1) % 2;

        if data.is_empty() {
            None
        } else {
            Some(data)
        }
    }

    pub fn resize(&mut self, gl: &glow::Context, width: u32, height: u32) {
        self.width = width;
        self.height = height;
        self.frame_count = 0;
        let size = (width * height * 4) as i32;
        unsafe {
            for pbo in &self.pbos {
                gl.delete_buffer(*pbo);
            }
            self.pbos = [gl.create_buffer().unwrap(), gl.create_buffer().unwrap()];
            for pbo in &self.pbos {
                gl.bind_buffer(glow::PIXEL_PACK_BUFFER, Some(*pbo));
                gl.buffer_data_size(glow::PIXEL_PACK_BUFFER, size, glow::STREAM_READ);
            }
            gl.bind_buffer(glow::PIXEL_PACK_BUFFER, None);
        }
    }

    pub fn destroy(&self, gl: &glow::Context) {
        unsafe {
            for pbo in &self.pbos {
                gl.delete_buffer(*pbo);
            }
            gl.delete_framebuffer(self.fbo);
        }
    }

    /// Drain all remaining frames: first flush PBO pipeline, then capture final frame.
    /// The double-buffered PBO has up to 2 frames queued. We drain those via end_frame(),
    /// then do a final synchronous glReadPixels to capture the latest texture content.
    pub fn flush(&mut self, gl: &glow::Context, src_tex: glow::Texture) -> Vec<Vec<u8>> {
        let mut frames = Vec::new();

        // Drain any buffered frames in the PBO pipeline (up to 2 frames in double-buffered setup)
        for _ in 0..3 {
            if let Some(data) = self.end_frame(gl) {
                if !data.iter().all(|&b| b == 0) {
                    frames.push(data);
                }
            }
        }

        // Final synchronous read of the latest texture content to ensure we get
        // the last frame that may still be on the source texture but not yet
        // in the PBO pipeline.
        let size = (self.width * self.height * 4) as usize;
        let mut pixels = vec![0u8; size];
        unsafe {
            gl.bind_framebuffer(glow::READ_FRAMEBUFFER, Some(self.fbo));
            gl.framebuffer_texture_2d(
                glow::READ_FRAMEBUFFER,
                glow::COLOR_ATTACHMENT0,
                glow::TEXTURE_2D,
                Some(src_tex),
                0,
            );
            gl.read_pixels(
                0,
                0,
                self.width as i32,
                self.height as i32,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(Some(&mut pixels[..])),
            );
            gl.bind_framebuffer(glow::READ_FRAMEBUFFER, None);
        }
        if !pixels.iter().all(|&b| b == 0) {
            frames.push(pixels);
        }

        self.frame_count = 0;
        frames
    }
}
