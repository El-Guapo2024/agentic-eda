//! The Rust types of KiCad's IPC API, made by prost-build from the `.proto` files of the KiCad tree kicad-cli is built from
//! (`api/proto/**.proto`, 25ec2335). The files in `gen/` are generated (`crates/kicad-engine/api-gen`) and committed, so building this crate needs
//! neither protoc nor prost-build; the module tree below is the protobuf packages' (`kiapi.board.commands` is [`kiapi::board::commands`]), which the
//! generated code's `super::` paths rely on. The well-known types KiCad imports (`google.protobuf.Any`, `FieldMask`) are generated too; `Empty`, which
//! KiCad answers many commands with, is prost's `()`.
#![allow(clippy::all, missing_docs, rustdoc::all)]

pub mod google {
    pub mod protobuf {
        include!("gen/google.protobuf.rs");
    }
}

pub mod kiapi {
    pub mod common {
        include!("gen/kiapi.common.rs");
        pub mod types {
            include!("gen/kiapi.common.types.rs");
        }
        pub mod commands {
            include!("gen/kiapi.common.commands.rs");
        }
        pub mod project {
            include!("gen/kiapi.common.project.rs");
        }
    }
    pub mod board {
        include!("gen/kiapi.board.rs");
        pub mod types {
            include!("gen/kiapi.board.types.rs");
        }
        pub mod commands {
            include!("gen/kiapi.board.commands.rs");
        }
        pub mod jobs {
            include!("gen/kiapi.board.jobs.rs");
        }
    }
    pub mod schematic {
        include!("gen/kiapi.schematic.rs");
        pub mod types {
            include!("gen/kiapi.schematic.types.rs");
        }
        pub mod commands {
            include!("gen/kiapi.schematic.commands.rs");
        }
        pub mod jobs {
            include!("gen/kiapi.schematic.jobs.rs");
        }
    }
}
