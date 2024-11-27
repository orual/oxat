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
