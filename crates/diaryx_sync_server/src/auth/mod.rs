mod magic_link;
mod middleware;

pub use magic_link::{MagicLinkError, MagicLinkService};
pub use middleware::{
    AuthExtractor, AuthUser, OptionalAuth, RequireAuth, extract_token_from_query, validate_token,
};
