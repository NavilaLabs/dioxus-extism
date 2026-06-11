use dioxus::prelude::*;
use dioxus_extism_protocol::SessionId;
use uuid::Uuid;

/// Provides a `Signal<SessionId>` in context.
///
/// Wrap your application root with this component — or with a concrete provider
/// like [`WebSessionProvider`], [`DesktopSessionProvider`], or [`MobileSessionProvider`].
/// All plugin-aware components read the session ID via [`use_session_id`].
#[component]
pub fn SessionProviderRoot<P: 'static + Clone + PartialEq>(
    provider: P,
    children: Element,
) -> Element {
    let _ = provider;
    let session_id: Signal<SessionId> = use_signal(|| SessionId(Uuid::new_v4().to_string()));
    provide_context(session_id);
    rsx! { {children} }
}

/// Session provider for web (WASM) targets — persists the session ID in `localStorage`.
///
/// A new UUID is generated on first visit and reused on subsequent page loads.
#[derive(Clone, PartialEq, Eq)]
pub struct WebSessionProvider;

/// Session provider for desktop targets — persists the session ID in a file under
/// the platform's local data directory. Uses `fd-lock` to prevent race conditions.
#[derive(Clone, PartialEq, Eq)]
pub struct DesktopSessionProvider;

/// Session provider for mobile targets — persists the session ID in the OS keychain
/// or keystore via the `keyring` crate. Survives app reinstall.
///
/// # Arguments
/// * `service` — a unique name for your app, used as the keyring service identifier
///   (e.g. `"com.example.myapp"`).
///
/// # Platform support
/// - **iOS / macOS**: Keychain Services
/// - **Android**: Android Keystore
/// - **Linux**: libsecret / `KWallet`
/// - **Windows**: Credential Manager
///
/// Only available on non-WASM targets.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, PartialEq, Eq)]
pub struct MobileSessionProvider {
    service: String,
}

#[cfg(not(target_arch = "wasm32"))]
impl MobileSessionProvider {
    const KEYRING_USER: &'static str = "dioxus-extism-session";

    /// Create a new provider with the given service name.
    pub fn new(service: impl Into<String>) -> Self {
        Self { service: service.into() }
    }

    /// Retrieve the existing session ID from the keychain, or create and store a new one.
    ///
    /// Logs a warning and falls back to a fresh in-memory UUID if the keychain is unavailable.
    pub fn session_id(&self) -> SessionId {
        let entry = keyring::Entry::new(&self.service, Self::KEYRING_USER);
        match entry {
            Ok(e) => match e.get_password() {
                Ok(stored) if !stored.is_empty() => SessionId(stored),
                _ => {
                    let id = Uuid::new_v4().to_string();
                    if let Ok(e) = keyring::Entry::new(&self.service, Self::KEYRING_USER)
                        && let Err(err) = e.set_password(&id)
                    {
                        tracing::warn!("MobileSessionProvider: keychain write failed: {err}");
                    }
                    SessionId(id)
                }
            },
            Err(err) => {
                tracing::warn!("MobileSessionProvider: keychain unavailable: {err}");
                SessionId(Uuid::new_v4().to_string())
            }
        }
    }
}

/// Read the current `SessionId` from context.
#[must_use]
pub fn use_session_id() -> Signal<SessionId> {
    use_context::<Signal<SessionId>>()
}
