#[derive(Debug, Clone)]
pub struct Parameter {
    pub name: &'static str,
    pub description: &'static str,
    pub optional: bool,
    pub default: Option<&'static str>,
}

#[derive(Debug, Clone)]
pub struct XrpcCommand {
    pub method: &'static str,
    pub description: &'static str,
    pub parameters: &'static [Parameter],
}

pub const AVAILABLE_COMMANDS: &[XrpcCommand] = &[
    XrpcCommand {
        method: "app.bsky.actor.getProfile",
        description: "Get an actor's profile details",
        parameters: &[Parameter {
            name: "actor",
            description: "The handle or DID of the actor",
            optional: false,
            default: None,
        }],
    },
    XrpcCommand {
        method: "app.bsky.feed.getTimeline",
        description: "Get the user's home timeline",
        parameters: &[
            Parameter {
                name: "limit",
                description: "Number of results to return",
                optional: true,
                default: Some("50"),
            },
            Parameter {
                name: "cursor",
                description: "Pagination cursor from previous response",
                optional: true,
                default: None,
            },
        ],
    },
    XrpcCommand {
        method: "com.atproto.identity.resolveHandle",
        description: "Resolve a handle (domain name) to a DID",
        parameters: &[
            Parameter {
                name: "handle",
                description: "The handle to resolve",
                optional: false,
                default: None,
            },
        ],
    },
    XrpcCommand {
        method: "app.bsky.feed.getPostThread",
        description: "Get a thread of posts by a post URI",
        parameters: &[
            Parameter {
                name: "uri",
                description: "The URI of the post used as entry point",
                optional: false,
                default: None,
            },
            Parameter {
                name: "depth",
                description: "How many levels of reply depth should be included in the response",
                optional: true,
                default: Some("6"),
            },
            Parameter {
                name: "parentHeight",
                description: "How many levels of parent (and grandparent, etc) post to include",
                optional: true,
                default: Some("80"),
            },
        ],
    },
    XrpcCommand {
        method: "app.bsky.feed.getAuthorFeed",
        description: "Get a feed of posts by an actor",
        parameters: &[
            Parameter {
                name: "actor",
                description: "The handle or DID of the author",
                optional: false,
                default: None,
            },
            Parameter {
                name: "limit",
                description: "Number of results",
                optional: true,
                default: Some("50"),
            },
            Parameter {
                name: "cursor",
                description: "Pagination cursor",
                optional: true,
                default: None,
            },
        ],
    },
    XrpcCommand {
        method: "app.bsky.graph.getFollowers",
        description: "Get a list of an actor's followers",
        parameters: &[
            Parameter {
                name: "actor",
                description: "The handle or DID of the actor",
                optional: false,
                default: None,
            },
            Parameter {
                name: "limit",
                description: "Number of results",
                optional: true,
                default: Some("50"),
            },
            Parameter {
                name: "cursor",
                description: "Pagination cursor",
                optional: true,
                default: None,
            },
        ],
    },
];
