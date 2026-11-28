//! Rasterize a selected page inside the bounded PDF worker.
use minidora_domain::{Result, failure};

#[cfg(windows)]
pub(crate) const MAX_PNG_BYTES: usize = 6 * 1024 * 1024;

pub(crate) fn rasterize(bytes: &[u8], page_number: u32) -> Result<Vec<u8>> {
    #[cfg(windows)]
    {
        native::rasterize(bytes, page_number)
    }
    #[cfg(not(windows))]
    {
        let _ = (bytes, page_number);
        Err(failure(
            "PDF_RASTER_UNAVAILABLE",
            "走査PDFのページ画像化は現在Windowsで利用できます。",
        ))
    }
}

#[cfg(windows)]
mod native {
    use super::{MAX_PNG_BYTES, Result, failure};
    use minidora_domain::{Error, MAX_PDF_ATTACHMENT_BYTES};
    use std::sync::mpsc;
    use windows::{
        Data::Pdf::{PdfDocument, PdfPage, PdfPageRenderOptions},
        Graphics::Imaging::BitmapEncoder,
        Storage::Streams::{DataReader, DataWriter, InMemoryRandomAccessStream},
        System::{DispatcherQueueController, DispatcherQueueHandler},
    };

    const MAX_EDGE: u32 = 1_800;
    const INITIAL_LONG_EDGE_DIPS: u32 = 1_200;

    fn invalid_document() -> Error {
        failure(
            "PDF_RASTER_INVALID",
            "走査PDFのページを読み取れませんでした。",
        )
    }

    fn invalid_page() -> Error {
        failure("PDF_RASTER_PAGE", "指定されたPDFページを照合できません。")
    }

    fn render_failed() -> Error {
        failure(
            "PDF_RASTER_RENDER_FAILED",
            "PDFページの画像化を完了できませんでした。",
        )
    }

    fn resource_limit() -> Error {
        failure(
            "PDF_RASTER_LIMIT",
            "PDFページの画像化に使う量が上限を超えました。",
        )
    }

    fn input_stream(bytes: &[u8]) -> Result<InMemoryRandomAccessStream> {
        let inner = || -> windows::core::Result<_> {
            let stream = InMemoryRandomAccessStream::new()?;
            let writer = DataWriter::CreateDataWriter(&stream.GetOutputStreamAt(0)?)?;
            writer.WriteBytes(bytes)?;
            let written = writer.StoreAsync()?.join()?;
            let _ = writer.DetachStream()?;
            Ok((stream, written))
        };
        let (stream, written) = inner().map_err(|_| invalid_document())?;
        if written as usize != bytes.len() {
            return Err(invalid_document());
        }
        stream.Seek(0).map_err(|_| invalid_document())?;
        Ok(stream)
    }

    fn render(page: &PdfPage, landscape: bool, dips: u32) -> Result<Vec<u8>> {
        let render_png = || -> windows::core::Result<_> {
            let stream = InMemoryRandomAccessStream::new()?;
            let options = PdfPageRenderOptions::new()?;
            options.SetBitmapEncoderId(BitmapEncoder::PngEncoderId()?)?;
            options.SetIsIgnoringHighContrast(true)?;
            if landscape {
                options.SetDestinationWidth(dips)?;
            } else {
                options.SetDestinationHeight(dips)?;
            }
            page.RenderWithOptionsToStreamAsync(&stream, &options)?
                .join()?;
            Ok(stream)
        };
        let stream = render_png().map_err(|_| render_failed())?;
        let size = stream.Size().map_err(|_| render_failed())?;
        if size > MAX_PNG_BYTES as u64 {
            return Err(resource_limit());
        }
        if size < 24 {
            return Err(render_failed());
        }
        let read_png = || -> windows::core::Result<_> {
            let reader = DataReader::CreateDataReader(&stream.GetInputStreamAt(0)?)?;
            let loaded = reader.LoadAsync(size as u32)?.join()?;
            let mut png = vec![0; loaded as usize];
            reader.ReadBytes(&mut png)?;
            let _ = reader.DetachStream()?;
            Ok(png)
        };
        let png = read_png().map_err(|_| render_failed())?;
        if png.len() as u64 != size {
            return Err(render_failed());
        }
        Ok(png)
    }

    fn png_size(png: &[u8]) -> Result<(u32, u32)> {
        let [
            137,
            80,
            78,
            71,
            13,
            10,
            26,
            10,
            0,
            0,
            0,
            13,
            b'I',
            b'H',
            b'D',
            b'R',
            w0,
            w1,
            w2,
            w3,
            h0,
            h1,
            h2,
            h3,
            ..,
        ] = png
        else {
            return Err(render_failed());
        };
        let width = u32::from_be_bytes([*w0, *w1, *w2, *w3]);
        let height = u32::from_be_bytes([*h0, *h1, *h2, *h3]);
        if width == 0 || height == 0 {
            return Err(render_failed());
        }
        Ok((width, height))
    }

    pub(super) fn rasterize(bytes: &[u8], page_number: u32) -> Result<Vec<u8>> {
        if bytes.is_empty() || bytes.len() > MAX_PDF_ATTACHMENT_BYTES {
            return Err(resource_limit());
        }
        let index = page_number.checked_sub(1).ok_or_else(invalid_page)?;
        // The Windows PDF renderer can crash on process shutdown in an MTA.
        // Own a dedicated STA and finish shutting it down before accepting bytes.
        let controller =
            DispatcherQueueController::CreateOnDedicatedThread().map_err(|_| render_failed())?;
        let result = rasterize_on_dispatcher(&controller, bytes.to_vec(), index);
        controller
            .ShutdownQueueAsync()
            .and_then(|operation| operation.join())
            .map_err(|_| render_failed())?;
        result
    }

    fn rasterize_on_dispatcher(
        controller: &DispatcherQueueController,
        bytes: Vec<u8>,
        index: u32,
    ) -> Result<Vec<u8>> {
        let (sender, receiver) = mpsc::channel();
        let handler = DispatcherQueueHandler::new(move || {
            let _ = sender.send(rasterize_page(&bytes, index));
            Ok(())
        });
        let queue = controller.DispatcherQueue().map_err(|_| render_failed())?;
        if !queue.TryEnqueue(&handler).map_err(|_| render_failed())? {
            return Err(render_failed());
        }
        // The parent Job watchdog bounds both rendering and dispatcher shutdown.
        receiver.recv().map_err(|_| render_failed())?
    }

    fn rasterize_page(bytes: &[u8], index: u32) -> Result<Vec<u8>> {
        let source = input_stream(bytes)?;
        let document = PdfDocument::LoadFromStreamAsync(&source)
            .and_then(|operation| operation.join())
            .map_err(|_| invalid_document())?;
        let count = document.PageCount().map_err(|_| invalid_document())?;
        if count as usize > crate::pdf_worker::MAX_PAGES {
            return Err(resource_limit());
        }
        if index >= count {
            return Err(invalid_page());
        }
        let page = document.GetPage(index).map_err(|_| invalid_page())?;
        let size = page.Size().map_err(|_| invalid_page())?;
        let (width, height) = (f64::from(size.Width), f64::from(size.Height));
        if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
            return Err(invalid_page());
        }

        let mut dips = INITIAL_LONG_EDGE_DIPS;
        // WinRT's requested dimensions are DIPs. Check the actual PNG pixels
        // and allow one smaller render when display scaling exceeds our cap.
        for _ in 0..2 {
            let png = render(&page, width >= height, dips)?;
            let (pixel_width, pixel_height) = png_size(&png)?;
            let longest = pixel_width.max(pixel_height);
            if longest <= MAX_EDGE {
                let expected_height = f64::from(pixel_width) * height / width;
                let expected_width = f64::from(pixel_height) * width / height;
                let deviation = if width >= height {
                    (f64::from(pixel_height) - expected_height).abs()
                } else {
                    (f64::from(pixel_width) - expected_width).abs()
                };
                if deviation > 2.0 {
                    return Err(render_failed());
                }
                return Ok(png);
            }
            dips = ((u64::from(dips) * u64::from(MAX_EDGE - 1)) / u64::from(longest)).max(1) as u32;
        }
        Err(resource_limit())
    }
}
