//! `ProviderKind` -> `dyn SystemProvider` lookup table.

use std::collections::BTreeMap;

use shared_types::{AppError, AppResult, ProviderKind, SystemProvider};

/// Registry of the system adapters available to the engine. Adding a new
/// subsystem means registering a provider here; the engine does not change.
#[derive(Default)]
pub struct ProviderRegistry {
    providers: BTreeMap<ProviderKind, Box<dyn SystemProvider>>,
}

impl std::fmt::Debug for ProviderRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderRegistry")
            .field("kinds", &self.providers.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl ProviderRegistry {
    /// Creates an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers `provider` under its [`SystemProvider::kind`].
    ///
    /// # Errors
    /// [`AppError::Internal`] if a provider of the same kind is already registered.
    pub fn register(&mut self, provider: Box<dyn SystemProvider>) -> AppResult<()> {
        let kind = provider.kind();
        if self.providers.contains_key(&kind) {
            return Err(AppError::Internal {
                message: format!("provider `{kind}` is already registered"),
            });
        }
        self.providers.insert(kind, provider);
        Ok(())
    }

    /// Looks up the provider for `kind`.
    ///
    /// # Errors
    /// [`AppError::ProviderNotRegistered`] if none was registered.
    pub fn get(&self, kind: ProviderKind) -> AppResult<&dyn SystemProvider> {
        self.providers
            .get(&kind)
            .map(|p| p.as_ref())
            .ok_or(AppError::ProviderNotRegistered { provider: kind })
    }

    /// Registered kinds in a stable order.
    pub fn kinds(&self) -> Vec<ProviderKind> {
        self.providers.keys().copied().collect()
    }
}
