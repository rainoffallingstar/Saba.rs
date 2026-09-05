//! Aggregated native text-input state.
//!
//! The shell previously kept 16 `NativeTextInput` + 17 `FocusHandle` fields as
//! flat `ShellApp` fields. This struct bundles them so the shell's state stays
//! readable and the single access point (`ShellApp::active_text_input_mut`)
//! owns every text buffer uniformly. `ActiveTextInput` is the shell's enum
//! naming a specific input; `TextInputs` maps that enum to a mutable buffer.

use gpui_kit::{App, FocusHandle, SharedString};

use crate::ActiveTextInput;
use crate::native_text_input::NativeTextInput;

/// All editable text buffers and their focus handles, grouped by domain.
pub struct TextInputs {
    // Engine / GTP
    pub gtp_input: NativeTextInput,
    pub engine_input_focus_handle: FocusHandle,
    pub engine_spec_input: NativeTextInput,
    pub engine_spec_focus_handle: FocusHandle,
    pub engine_draft: SharedString,
    // Fox / live
    pub fox_query_input: NativeTextInput,
    pub fox_query_focus_handle: FocusHandle,
    pub live_url_input: NativeTextInput,
    pub live_url_focus_handle: FocusHandle,
    // OGS
    pub ogs_username_input: NativeTextInput,
    pub ogs_password_input: NativeTextInput,
    pub ogs_username_focus_handle: FocusHandle,
    pub ogs_password_focus_handle: FocusHandle,
    pub ogs_game_id_input: NativeTextInput,
    pub ogs_game_id_focus_handle: FocusHandle,
    pub ogs_chat_input: NativeTextInput,
    pub ogs_chat_focus_handle: FocusHandle,
    // Library
    pub library_id_input: NativeTextInput,
    pub library_id_focus_handle: FocusHandle,
    pub library_name_input: NativeTextInput,
    pub library_name_focus_handle: FocusHandle,
    pub library_github_url_input: NativeTextInput,
    pub library_github_url_focus_handle: FocusHandle,
    pub library_reference_input: NativeTextInput,
    pub library_reference_focus_handle: FocusHandle,
    pub library_license_name_input: NativeTextInput,
    pub library_license_name_focus_handle: FocusHandle,
    pub library_license_url_input: NativeTextInput,
    pub library_license_url_focus_handle: FocusHandle,
    // Settings
    pub settings_input_focus_handle: FocusHandle,
    pub settings_draft: SharedString,
    // Comment / node title
    pub comment_focus_handle: FocusHandle,
    pub comment_input: NativeTextInput,
    pub node_title_focus_handle: FocusHandle,
    pub node_title_input: NativeTextInput,
}

impl TextInputs {
    /// Builds every buffer empty and every focus handle fresh.
    pub fn new(cx: &mut App) -> Self {
        Self {
            gtp_input: NativeTextInput::new(""),
            engine_input_focus_handle: cx.focus_handle(),
            engine_spec_input: NativeTextInput::new(""),
            engine_spec_focus_handle: cx.focus_handle(),
            engine_draft: SharedString::default(),
            fox_query_input: NativeTextInput::new(""),
            fox_query_focus_handle: cx.focus_handle(),
            live_url_input: NativeTextInput::new(""),
            live_url_focus_handle: cx.focus_handle(),
            ogs_username_input: NativeTextInput::new(""),
            ogs_password_input: NativeTextInput::new(""),
            ogs_username_focus_handle: cx.focus_handle(),
            ogs_password_focus_handle: cx.focus_handle(),
            ogs_game_id_input: NativeTextInput::new(""),
            ogs_game_id_focus_handle: cx.focus_handle(),
            ogs_chat_input: NativeTextInput::new(""),
            ogs_chat_focus_handle: cx.focus_handle(),
            library_id_input: NativeTextInput::new(""),
            library_id_focus_handle: cx.focus_handle(),
            library_name_input: NativeTextInput::new(""),
            library_name_focus_handle: cx.focus_handle(),
            library_github_url_input: NativeTextInput::new(""),
            library_github_url_focus_handle: cx.focus_handle(),
            library_reference_input: NativeTextInput::new("main"),
            library_reference_focus_handle: cx.focus_handle(),
            library_license_name_input: NativeTextInput::new(""),
            library_license_name_focus_handle: cx.focus_handle(),
            library_license_url_input: NativeTextInput::new(""),
            library_license_url_focus_handle: cx.focus_handle(),
            settings_input_focus_handle: cx.focus_handle(),
            settings_draft: SharedString::default(),
            comment_focus_handle: cx.focus_handle(),
            comment_input: NativeTextInput::new(""),
            node_title_focus_handle: cx.focus_handle(),
            node_title_input: NativeTextInput::new(""),
        }
    }

    /// Resolves the active input to its mutable text buffer.
    pub fn active_mut(&mut self, active: Option<ActiveTextInput>) -> Option<&mut NativeTextInput> {
        match active {
            Some(ActiveTextInput::Comment) => Some(&mut self.comment_input),
            Some(ActiveTextInput::NodeTitle) => Some(&mut self.node_title_input),
            Some(ActiveTextInput::FoxQuery) => Some(&mut self.fox_query_input),
            Some(ActiveTextInput::LiveUrl) => Some(&mut self.live_url_input),
            Some(ActiveTextInput::LibraryId) => Some(&mut self.library_id_input),
            Some(ActiveTextInput::LibraryName) => Some(&mut self.library_name_input),
            Some(ActiveTextInput::LibraryGithubUrl) => Some(&mut self.library_github_url_input),
            Some(ActiveTextInput::LibraryReference) => Some(&mut self.library_reference_input),
            Some(ActiveTextInput::LibraryLicenseName) => Some(&mut self.library_license_name_input),
            Some(ActiveTextInput::LibraryLicenseUrl) => Some(&mut self.library_license_url_input),
            Some(ActiveTextInput::GtpInput) => Some(&mut self.gtp_input),
            Some(ActiveTextInput::EngineSpec) => Some(&mut self.engine_spec_input),
            Some(ActiveTextInput::OgsUsername) => Some(&mut self.ogs_username_input),
            Some(ActiveTextInput::OgsPassword) => Some(&mut self.ogs_password_input),
            Some(ActiveTextInput::OgsGameId) => Some(&mut self.ogs_game_id_input),
            Some(ActiveTextInput::OgsChat) => Some(&mut self.ogs_chat_input),
            None => None,
        }
    }

    /// Resolves the active input to its immutable text buffer.
    pub fn active(&self, active: Option<ActiveTextInput>) -> Option<&NativeTextInput> {
        match active {
            Some(ActiveTextInput::Comment) => Some(&self.comment_input),
            Some(ActiveTextInput::NodeTitle) => Some(&self.node_title_input),
            Some(ActiveTextInput::FoxQuery) => Some(&self.fox_query_input),
            Some(ActiveTextInput::LiveUrl) => Some(&self.live_url_input),
            Some(ActiveTextInput::LibraryId) => Some(&self.library_id_input),
            Some(ActiveTextInput::LibraryName) => Some(&self.library_name_input),
            Some(ActiveTextInput::LibraryGithubUrl) => Some(&self.library_github_url_input),
            Some(ActiveTextInput::LibraryReference) => Some(&self.library_reference_input),
            Some(ActiveTextInput::LibraryLicenseName) => Some(&self.library_license_name_input),
            Some(ActiveTextInput::LibraryLicenseUrl) => Some(&self.library_license_url_input),
            Some(ActiveTextInput::GtpInput) => Some(&self.gtp_input),
            Some(ActiveTextInput::EngineSpec) => Some(&self.engine_spec_input),
            Some(ActiveTextInput::OgsUsername) => Some(&self.ogs_username_input),
            Some(ActiveTextInput::OgsPassword) => Some(&self.ogs_password_input),
            Some(ActiveTextInput::OgsGameId) => Some(&self.ogs_game_id_input),
            Some(ActiveTextInput::OgsChat) => Some(&self.ogs_chat_input),
            None => None,
        }
    }
}
