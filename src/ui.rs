use std::marker::PhantomData;

/// The application-owned Molerat integration handle.
#[derive(Debug)]
pub struct Molerat<M = ()> {
    /// The application name exposed to accessibility and MCP consumers.
    name: String,
    /// Records which message type this handle will emit.
    ///
    /// No message is stored yet. The `fn() -> M` marker means "produces `M`" without making the
    /// handle behave as if it owns an `M`.
    message: PhantomData<fn() -> M>,
}

impl<M> Molerat<M> {
    /// Creates an integration handle for an application using message type `M`.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            message: PhantomData,
        }
    }

    /// Returns the application name exposed to consumers.
    pub fn name(&self) -> &str {
        &self.name
    }
}

impl Molerat {
    /// Creates an integration handle that only exposes application state.
    pub fn read_only(name: impl Into<String>) -> Self {
        Self::new(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_interactive_instance() {
        enum Message {}

        let molerat = Molerat::<Message>::new("example");

        assert_eq!(molerat.name(), "example");
    }

    #[test]
    fn creates_read_only_instance() {
        let molerat = Molerat::read_only("example");

        assert_eq!(molerat.name(), "example");
    }
}
