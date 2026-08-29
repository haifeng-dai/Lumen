//! 跨部件共享的纯函数工具。不依赖 `PdfReaderView` 自身状态，仅操作传入参数。
//! 用于消除 page / actions / pip 之间的代码重复。

use std::sync::Arc;

use crate::pdf::PAGE_BASE_WIDTH_REMS;

/// 将 muPDF 输出的 RgbaImage 包装为 GPUI 可渲染的 ImageSource。
/// 三段重复代码（cache_page_image / cache_thumbnail_image / PinRendered）统一调此函数。
pub fn make_image_source(raw: image::RgbaImage) -> gpui::ImageSource {
    let frame = image::Frame::new(raw);
    let render_img = gpui::RenderImage::new(smallvec::smallvec![frame]);
    gpui::ImageSource::Render(Arc::new(render_img))
}

/// 将 PDF 渲染缓存中的 BGRA 图片转换为标准 RGBA 图片。
///
/// `RgbaImage` 是现有缓存使用的类型名，但 MuPDF 的 `device_bgr()` 输出
/// 实际上按 BGRA 字节排列。该函数复制输入并只交换 R/B，避免污染仍供
/// GPUI 显示的原始缓存；G/A、尺寸和透明度语义保持不变。
pub(crate) fn bgra_to_rgba(img: &image::RgbaImage) -> image::RgbaImage {
    let mut converted = img.clone();
    for pixel in converted.pixels_mut() {
        pixel.0.swap(0, 2);
    }
    converted
}

/// 将 PDF 渲染缓存中的 BGRA 图片转换后写入系统剪贴板。
pub fn copy_bgra_to_clipboard(img: &image::RgbaImage) {
    use arboard::Clipboard;
    let rgba = bgra_to_rgba(img);
    if let Ok(mut cb) = Clipboard::new() {
        cb.set_image(arboard::ImageData {
            width: rgba.width() as usize,
            height: rgba.height() as usize,
            bytes: std::borrow::Cow::from(rgba.as_raw().clone()),
        })
        .ok();
    }
}

#[cfg(test)]
mod tests {
    use super::bgra_to_rgba;
    use image::ImageBuffer;

    #[test]
    fn bgra_to_rgba_swaps_red_and_blue() {
        let input = ImageBuffer::from_raw(1, 1, vec![10, 20, 30, 40]).unwrap();
        let output = bgra_to_rgba(&input);
        assert_eq!(output.as_raw(), &[30, 20, 10, 40]);
        assert_eq!(input.as_raw(), &[10, 20, 30, 40]);
    }

    #[test]
    fn bgra_to_rgba_converts_each_pixel_without_reordering() {
        let input = ImageBuffer::from_raw(2, 1, vec![1, 2, 3, 4, 5, 6, 7, 8]).unwrap();
        let output = bgra_to_rgba(&input);
        assert_eq!(output.as_raw(), &[3, 2, 1, 4, 7, 6, 5, 8]);
        assert_eq!(input.as_raw(), &[1, 2, 3, 4, 5, 6, 7, 8]);
    }
}

/// 计算页面的显示尺寸（逻辑像素）。
/// `page_sizes` 存储 PDF 物理宽高，`page_index` 取值索引。
/// `rem_size` 为当前窗口 rem 像素值。
/// 返回 `(display_width_px, display_height_px)`。
pub fn page_display_size(
    page_sizes: &[(f32, f32)],
    page_index: usize,
    zoom_level: f32,
    rem_size: f32,
) -> (f32, f32) {
    let w = PAGE_BASE_WIDTH_REMS * zoom_level * rem_size;
    let h = page_height(page_sizes, page_index, zoom_level, rem_size);
    (w, h)
}

/// 仅计算页面的显示高度。适用于滚动条、滚动定位等只需 height 的场景。
pub fn page_height(
    page_sizes: &[(f32, f32)],
    page_index: usize,
    zoom_level: f32,
    rem_size: f32,
) -> f32 {
    let (pdf_w, pdf_h) = page_sizes
        .get(page_index)
        .copied()
        .unwrap_or((612.0, 792.0));
    (PAGE_BASE_WIDTH_REMS * zoom_level * rem_size) * (pdf_h / pdf_w)
}
