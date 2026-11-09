//! Device-backed form state shared by settings tabs.

use std::future::Future;

use dioxus::prelude::*;

/// `saved` mirrors the device; `form` holds unsaved edits. Tabs disable inputs while
/// `busy`, so a finished operation never overwrites edits made in the meantime.
pub struct Editable<T: 'static> {
    pub saved: Signal<Option<T>>,
    pub form: Signal<Option<T>>,
    pub busy: Signal<bool>,
    pub error: Signal<Option<String>>,
}

impl<T: 'static> Clone for Editable<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: 'static> Copy for Editable<T> {}

pub fn use_editable<T: 'static>() -> Editable<T> {
    Editable {
        saved: use_signal(|| None),
        form: use_signal(|| None),
        busy: use_signal(|| false),
        error: use_signal(|| None),
    }
}

impl<T: Clone + PartialEq + 'static> Editable<T> {
    pub fn dirty(&self) -> bool {
        *self.saved.read() != *self.form.read()
    }

    /// Runs a device operation that returns the device's fresh state.
    pub async fn run(mut self, op: impl Future<Output = eg_device::Result<T>>) {
        self.busy.set(true);
        match op.await {
            Ok(v) => {
                self.saved.set(Some(v.clone()));
                self.form.set(Some(v));
                self.error.set(None);
            }
            Err(e) => self.error.set(Some(e.to_string())),
        }
        self.busy.set(false);
    }

    pub fn edit(mut self, change: impl FnOnce(&mut T)) {
        if let Some(v) = self.form.write().as_mut() {
            change(v);
        }
    }

    pub fn revert(mut self) {
        let saved = self.saved.read().clone();
        self.form.set(saved);
    }
}
