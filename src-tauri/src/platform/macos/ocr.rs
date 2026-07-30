//! Reading the text out of an image, macOS side
//! ([storage.md](../../../../docs/storage.md) §The chunk index).
//!
//! Vision has been in the box since 10.15 and is the better of the two
//! engines this app uses — notably on handwriting, which the Windows one
//! barely manages. Nothing to download, nothing to bundle, no permission
//! prompt: `VNImageRequestHandler` works on bytes we already own.
//!
//! Vision's API is synchronous when driven this way — `performRequests:`
//! returns once the work is done — so there is no completion handler and
//! nothing to keep alive across a callback.

use objc2::rc::Retained;
use objc2::AnyThread;
use objc2_foundation::{NSArray, NSData, NSDictionary};
use objc2_vision::{
    VNImageRequestHandler, VNRecognizeTextRequest, VNRecognizedTextObservation, VNRequest,
    VNRequestTextRecognitionLevel,
};

use crate::platform::types::Recognized;

/// Read the text in one image. Bytes rather than a path: the handler takes
/// an `NSData` directly, and file IO belongs above the seam.
pub fn recognize_text(bytes: &[u8]) -> Recognized {
    let request = VNRecognizeTextRequest::new();
    request.setRecognitionLevel(VNRequestTextRecognitionLevel::Accurate);
    // Language correction is what turns a plausible-looking character soup
    // into words; the cost is negligible beside the recognition itself.
    request.setUsesLanguageCorrection(true);

    let data = NSData::with_bytes(bytes);
    let options = NSDictionary::new();
    let handler = VNImageRequestHandler::initWithData_options(
        VNImageRequestHandler::alloc(),
        &data,
        &options,
    );

    let as_request: &VNRequest = &request;
    if let Err(e) = handler.performRequests_error(&NSArray::from_slice(&[as_request])) {
        // Vision refused this file — a truncated download, or a format the
        // decoder will not take. An answer about the image, not about the
        // engine, so the caller retires it rather than retrying forever.
        return Recognized::Failed(e.localizedDescription().to_string());
    }

    // Results of a text request are text observations by construction;
    // Vision types them as the base class and offers no checked downcast.
    let Some(results) = request.results() else {
        return Recognized::Text(String::new());
    };
    let observations: Retained<NSArray<VNRecognizedTextObservation>> =
        unsafe { Retained::cast_unchecked(results) };

    let mut lines: Vec<String> = Vec::new();
    for observation in &*observations {
        // One candidate is enough: the rest are lower-confidence readings
        // of the same line, and search wants the engine's best guess.
        if let Some(best) = observation.topCandidates(1).firstObject() {
            lines.push(best.string().to_string());
        }
    }
    let borrowed: Vec<&str> = lines.iter().map(String::as_str).collect();
    Recognized::Text(embral_notes::ocr::normalize(&borrowed))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What the engine actually does with a real image, run by hand:
    /// `EMBRAL_TEST_OCR_IMAGE=/path/to/slide.png
    ///  cargo test -p embral --lib ocr -- --ignored --nocapture`
    ///
    /// Point it at a screenshot of slides *and* at a photographed
    /// whiteboard — Vision handles both, and the handwriting result is
    /// worth seeing at least once.
    #[test]
    #[ignore = "needs EMBRAL_TEST_OCR_IMAGE pointing at an image file"]
    fn real_image_is_read() {
        let path = std::env::var("EMBRAL_TEST_OCR_IMAGE").expect("set EMBRAL_TEST_OCR_IMAGE");
        let bytes = std::fs::read(&path).expect("read the image");
        let started = std::time::Instant::now();
        let outcome = recognize_text(&bytes);
        eprintln!(
            "{} bytes in {:.0} ms",
            bytes.len(),
            started.elapsed().as_secs_f64() * 1000.0
        );
        match outcome {
            Recognized::Text(text) => {
                eprintln!("--- usable: {} ---\n{text}", embral_notes::ocr::is_usable(&text));
            }
            other => panic!("expected text, got {other:?}"),
        }
    }

    #[test]
    fn a_non_image_fails_rather_than_pretending() {
        // Refusing to decode is `Failed` — an answer about this file — not
        // `Unavailable`, which would leave it pending forever.
        match recognize_text(b"this is not an image") {
            Recognized::Failed(_) => {}
            other => panic!("expected a failure, got {other:?}"),
        }
    }
}
