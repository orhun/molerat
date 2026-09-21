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
    fn uses_unit_as_the_default_message_type() {
        let molerat: Molerat = Molerat::new("example");

        assert_eq!(molerat.name(), "example");
    }
}
