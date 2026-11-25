//! The session's image list as the runtime reports it: which images exist,
//! their numbering, and which numbers this panel already knows. The pane that
//! shows them and the preview that loads one both read from it, so it knows
//! neither.
use super::*;

#[derive(Clone, serde::Deserialize)]
pub(super) struct GalleryImage {
    pub(super) image_id: String,
    #[serde(default)]
    ordinal: Option<usize>,
    message_index: usize,
    timestamp: Option<String>,
    media_type: String,
}

impl GalleryImage {
    pub(super) fn transcript_image(&self, data: String) -> TranscriptImage {
        let mut image = TranscriptImage::new(
            self.media_type.clone(),
            data,
            Some(format!("[#{}]", self.image_id)),
        );
        image.session_image = self.ordinal.map(|ordinal| jcode_sdk::api::SessionImageRef {
            image_id: self.image_id.clone(),
            ordinal,
        });
        image
    }

    /// `Image #3 · message 2 · 14:05`, or the id when the runtime has not
    /// numbered the image yet.
    pub(super) fn row_label(&self) -> String {
        let name = self.ordinal.map_or_else(
            || self.image_id.clone(),
            |ordinal| format!("Image #{ordinal}"),
        );
        format!(
            "{name} · message {} · {}",
            self.message_index + 1,
            self.timestamp.as_deref().unwrap_or("time unavailable")
        )
    }
}

/// What the runtime answered to the last image-list request.
#[derive(Clone, Default)]
pub(super) struct Listing(Option<Result<Vec<GalleryImage>, String>>);

impl Listing {
    fn images(&self) -> Option<&Vec<GalleryImage>> {
        self.0.as_ref()?.as_ref().ok()
    }

    pub(super) fn is_loaded(&self) -> bool {
        self.0.is_some()
    }

    pub(super) fn has_images(&self) -> bool {
        self.images().is_some_and(|images| !images.is_empty())
    }

    pub(super) fn error(&self) -> Option<&str> {
        self.0.as_ref()?.as_ref().err().map(String::as_str)
    }

    pub(super) fn rows(&self) -> &[GalleryImage] {
        self.images().map_or(&[], Vec::as_slice)
    }
}

#[derive(Default)]
pub(super) struct Gallery {
    listing: Listing,
    live_ordinals: HashMap<String, usize>,
    requested_prompt_keys: std::collections::HashSet<String>,
    loading: bool,
}

impl Gallery {
    pub(super) fn receive(&mut self, result: Result<String, String>) {
        self.loading = false;
        self.listing = Listing(Some(result.and_then(|json| {
            serde_json::from_str(&json).map_err(|error| error.to_string())
        })));
    }

    /// Marks a list request as in flight. `false` when one already is.
    pub(super) fn begin_load(&mut self) -> bool {
        !std::mem::replace(&mut self.loading, true)
    }

    pub(super) fn listing(&self) -> &Listing {
        &self.listing
    }

    pub(super) fn loaded_image(&self, image_id: &str, data: &str) -> Option<TranscriptImage> {
        let image = self
            .listing
            .rows()
            .iter()
            .find(|image| image.image_id == image_id)?;
        Some(image.transcript_image(data.to_owned()))
    }

    pub(super) fn highest_ordinal(&self) -> Option<usize> {
        self.listing
            .rows()
            .iter()
            .filter_map(|image| image.ordinal)
            .max()
    }

    /// Numbers announced by live `UserImageIds` events, which arrive before
    /// the list does.
    pub(super) fn note_live_ordinals(&mut self, ids: &[String], ordinals: &[usize]) {
        self.live_ordinals.extend(
            ids.iter()
                .cloned()
                .zip(ordinals.iter().copied())
                .filter(|(_, ordinal)| *ordinal > 0),
        );
    }

    pub(super) fn may_request_prompt_key(&self, key: &str) -> bool {
        !self.requested_prompt_keys.contains(key)
    }

    /// Remembers the keys; `true` when any was new and the list must be asked again.
    pub(super) fn request_prompt_keys(&mut self, keys: Vec<String>) -> bool {
        keys.into_iter()
            .fold(false, |new, key| self.requested_prompt_keys.insert(key) | new)
    }

    fn ordinal(&self, id: &str) -> Option<usize> {
        self.live_ordinals
            .get(id)
            .copied()
            .or_else(|| {
                self.listing
                    .rows()
                    .iter()
                    .find(|image| image.image_id == id)?
                    .ordinal
            })
            .filter(|ordinal| *ordinal > 0)
    }
}

impl Panel {
    pub(super) fn note_session_image_ordinal(
        &mut self,
        ordinal: Option<usize>,
        cx: &mut Context<Self>,
    ) {
        if let Some(ordinal) = ordinal.filter(|&ordinal| ordinal > self.session_image_base) {
            self.session_image_base = ordinal;
            self.input
                .update(cx, |input, cx| input.set_session_image_base(ordinal, cx));
        }
    }

    pub(super) fn gallery_image_ordinal(&self, marker: &str) -> Option<usize> {
        self.session_image_gallery
            .ordinal(crate::image_chip::session_image_key(marker)?)
    }
}
