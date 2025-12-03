const FONT_DATA: &[u8] = include_bytes!("font5x7.raw");
pub const FONT_CHAR_WIDTH: usize = 5;
pub const FONT_CHAR_HEIGHT: usize = 7;
const FONT_CHARS_PER_ROW: usize = 16;

pub struct Font {
    pub scale: usize,
}

impl Default for Font {
    fn default() -> Self {
        Self { scale: 2 }
    }
}

impl Font {
    pub fn new(scale: usize) -> Self {
        Self { scale }
    }

    pub fn char_width(&self) -> usize {
        FONT_CHAR_WIDTH * self.scale
    }

    pub fn char_height(&self) -> usize {
        FONT_CHAR_HEIGHT * self.scale
    }

    pub fn text_width(&self, text: &str) -> usize {
        text.len() * self.char_width()
    }

    pub fn draw_char(
        &self,
        pixels: &mut [u32],
        stride: usize,
        x: usize,
        y: usize,
        ch: char,
        color: u32,
    ) {
        let idx = if ch.is_ascii() && ch >= ' ' {
            (ch as usize) - 32
        } else {
            0
        };

        let font_x = (idx % FONT_CHARS_PER_ROW) * FONT_CHAR_WIDTH;
        let font_y = (idx / FONT_CHARS_PER_ROW) * FONT_CHAR_HEIGHT;

        for cy in 0..FONT_CHAR_HEIGHT {
            for cx in 0..FONT_CHAR_WIDTH {
                let px = font_x + cx;
                let py = font_y + cy;
                let byte_idx = py * (FONT_CHARS_PER_ROW * FONT_CHAR_WIDTH) + px;

                if byte_idx < FONT_DATA.len() && FONT_DATA[byte_idx] > 127 {
                    for sy in 0..self.scale {
                        for sx in 0..self.scale {
                            let screen_x = x + cx * self.scale + sx;
                            let screen_y = y + cy * self.scale + sy;
                            let pixel_idx = screen_y * stride + screen_x;
                            if pixel_idx < pixels.len() {
                                pixels[pixel_idx] = color;
                            }
                        }
                    }
                }
            }
        }
    }

    pub fn draw_text(
        &self,
        pixels: &mut [u32],
        stride: usize,
        x: usize,
        y: usize,
        text: &str,
        color: u32,
    ) {
        for (i, ch) in text.chars().enumerate() {
            self.draw_char(pixels, stride, x + i * self.char_width(), y, ch, color);
        }
    }

    pub fn draw_text_right(
        &self,
        pixels: &mut [u32],
        stride: usize,
        right_x: usize,
        y: usize,
        text: &str,
        color: u32,
    ) {
        let width = self.text_width(text);
        if right_x >= width {
            self.draw_text(pixels, stride, right_x - width, y, text, color);
        }
    }
}
