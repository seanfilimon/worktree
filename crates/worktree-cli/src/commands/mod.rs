//! Command implementations, one module per command group.

pub mod access;
pub mod admin;
pub mod api;
pub mod archive;
pub mod attr;
pub mod audit;
pub mod auth;
pub mod blame;
pub mod branch;
pub mod cherry_pick;
pub mod ci;
pub mod completions;
pub mod compliance;
pub mod config;
pub mod config_as_code; // config-as-code (§16.3)
pub mod daemon;
pub mod deps;
pub mod diff;
pub mod doctor;
pub mod events;
pub mod fork;
pub mod fsck;
pub mod gc;
pub mod git;
pub mod grant;
pub mod history;
pub mod iam;
pub mod iam_common;
pub mod identity;
pub mod ignore;
pub mod inbox;
pub mod init;
pub mod invite;
pub mod license;
pub mod merge;
pub mod mfa;
pub mod mirror;
pub mod mr;
pub mod open;
pub mod owners;
pub mod policy;
pub mod protect;
pub mod rebase;
pub mod release;
pub mod remote;
pub mod role;
pub mod sa;
pub mod search;
pub mod server_common;
pub mod session;
pub mod snapshot;
pub mod sparse;
pub mod sso;
pub mod stash;
pub mod status;
pub mod sync;
pub mod synchronize;
pub mod tag;
pub mod team;
pub mod tenant;
pub mod todo;
pub mod token;
pub mod tree;
pub mod trees;
pub mod undo;
pub mod webhook;

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
        Command::PasswordReset(args) => auth::password_reset(ctx, args),
        Command::Push(args) => sync::push(ctx, args),
        Command::Fetch(args) => sync::fetch(ctx, args),
        Command::Pull(args) => sync::pull(ctx, args),
        Command::Clone(args) => sync::clone(ctx, args),
        Command::Stage(args) => sync::stage(ctx, args),
        Command::Unstage(args) => sync::unstage(ctx, args), // daemon (§13.11)
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
        Command::Todo(args) => todo::run(ctx, args),
        Command::Deps(args) => deps::run(ctx, args),
        Command::Release(args) => release::run(ctx, args),
        Command::Ci(args) => ci::run(ctx, args),
        Command::Access(args) => access::run(ctx, args),
        Command::Identity(args) => identity::run(ctx, args),
        Command::Webhook(args) => webhook::run(ctx, args),
        Command::Blame(args) => blame::run(ctx, args),
        Command::CherryPick(args) => cherry_pick::run(ctx, args),
        Command::Rebase(args) => rebase::run(ctx, args),
        Command::Stash(args) => stash::run(ctx, args),
        Command::Gc(args) => gc::run(ctx, args),
        Command::Fsck(args) => fsck::run(ctx, args),
        Command::Completions(args) => completions::run(ctx, args),
        Command::Doctor(args) => doctor::run(ctx, args),
        Command::Open(args) => open::run(ctx, args),
        Command::Api(args) => api::run(ctx, args),
        Command::Sparse(args) => sparse::run(ctx, args),
        Command::Sync(args) => synchronize::run(ctx, args),
        Command::Fork(args) => fork::run(ctx, args),
        Command::Undo(args) => undo::run(ctx, args),
        Command::Attr(args) => attr::run(ctx, args),
        Command::Compliance(args) => compliance::run(ctx, args),
        Command::Inbox(args) => inbox::run(ctx, args),
        Command::Mirror(args) => mirror::run(ctx, args),
        Command::Owners(args) => owners::run(ctx, args),
    }
}
