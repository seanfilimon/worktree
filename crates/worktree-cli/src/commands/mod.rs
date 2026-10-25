//! Command implementations, one module per command group.

pub mod admin;
pub mod archive;
pub mod audit;
pub mod auth;
pub mod branch;
pub mod config;
pub mod daemon;
pub mod diff;
pub mod events;
pub mod git;
pub mod grant;
pub mod history;
pub mod iam;
pub mod ignore;
pub mod init;
pub mod invite;
pub mod license;
pub mod merge;
pub mod mfa;
pub mod mr;
pub mod policy;
pub mod protect;
pub mod remote;
pub mod role;
pub mod sa;
pub mod search;
pub mod session;
pub mod snapshot;
pub mod sso;
pub mod status;
pub mod sync;
pub mod tag;
pub mod team;
pub mod tenant;
pub mod token;
pub mod tree;
pub mod trees;

use crate::cli::Command;
use crate::context::Context;
use crate::error::Result;

pub fn run(ctx: &Context, command: Command) -> Result<()> {
    match command {
        Command::Init(args) => init::run(ctx, args),
        Command::Status(args) => status::run(ctx, args),
        Command::Snapshot(args) => snapshot::run(ctx, args),
        Command::Log(args) => history::log(ctx, args),
        Command::Show(args) => history::show(ctx, args),
        Command::Reflog(args) => history::reflog(ctx, args),
        Command::Diff(args) => diff::run(ctx, args),
        Command::Branch(args) => branch::run(ctx, args),
        Command::Switch(args) => branch::switch(ctx, args),
        Command::Merge(args) => merge::run(ctx, args),
        Command::Revert(args) => merge::revert(ctx, args),
        Command::Restore(args) => merge::restore(ctx, args),
        Command::Tag(args) => tag::run(ctx, args),
        Command::Tree(args) => tree::run(ctx, args),
        Command::Ignore(args) => ignore::run(ctx, args),
        Command::Config(args) => config::run(ctx, args),
        Command::Remote(args) => remote::run(ctx, args),
        Command::Login(args) => auth::login(ctx, args),
        Command::Logout(args) => auth::logout(ctx, args),
        Command::Whoami(args) => auth::whoami(ctx, args),
        Command::Register(args) => auth::register(ctx, args),
        Command::Push(args) => sync::push(ctx, args),
        Command::Fetch(args) => sync::fetch(ctx, args),
        Command::Pull(args) => sync::pull(ctx, args),
        Command::Clone(args) => sync::clone(ctx, args),
        Command::Stage(args) => sync::stage(ctx, args),
        Command::Archive(args) => archive::run(ctx, args),
        Command::Git(args) => git::run(ctx, args),
        Command::Daemon(args) => daemon::run(ctx, args),
        Command::Tenant(args) => tenant::run(ctx, args),
        Command::Trees(args) => trees::run(ctx, args),
        Command::Mr(args) => mr::run(ctx, args),
        Command::Protect(args) => protect::run(ctx, args),
        Command::License(args) => license::run(ctx, args),
        Command::Audit(args) => audit::run(ctx, args),
        Command::Admin(args) => admin::run(ctx, args),
        Command::Search(args) => search::run(ctx, args),
        Command::Events(args) => events::run(ctx, args),
        Command::Team(args) => team::run(ctx, args),
        Command::Role(args) => role::run(ctx, args),
        Command::Policy(args) => policy::run(ctx, args),
        Command::Grant(args) => grant::run(ctx, args),
        Command::Sa(args) => sa::run(ctx, args),
        Command::Token(args) => token::run(ctx, args),
        Command::Session(args) => session::run(ctx, args),
        Command::Mfa(args) => mfa::run(ctx, args),
        Command::Invite(args) => invite::run(ctx, args),
        Command::Iam(args) => iam::run(ctx, args),
        Command::Sso(args) => sso::run(ctx, args),
    }
}
