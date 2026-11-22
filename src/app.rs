//! The commands: argument checks, which profile, and output.

use std::path::{Path, PathBuf};

use clap::parser::ValueSource;
use clap::{ArgMatches, CommandFactory, FromArgMatches};
use pk_cli_config::profile::{self, ProfileName, Profiles};
use pk_cli_config::ConfigStore;
use pk_cli_core::info::{AuthInfo, CliInfo};
use pk_cli_core::{dates, output, CliError};
use pk_cli_selfupdate::Updater;
use pk_med_records::{create_export_file, Finished, RecordsExport, Trailer};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::cli::*;
use crate::model::Test;
use crate::portal::{self, Env, Site};

const REPO: &str = "own-your-health/oneblood-cli";
pub const PROFILE_ENV: &str = "ONEBLOOD_PROFILE";
/// `info`'s profiles.
pub const PROFILES: &[&str] = &[pk_med_records::PROFILE];

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Config {
    /// The donor-portal logins, one per account (`oneblood profiles add <NAME>`).
    #[serde(default, skip_serializing_if = "Profiles::is_empty")]
    pub profiles: Profiles,
}

pub fn main() {
    // A reader that closes the pipe early (`oneblood readings list | head`)
    // ends the process quietly, as it does any Unix tool.
    #[cfg(unix)]
    // SAFETY: called first, before any thread starts; it only resets the
    // disposition Rust's runtime set for SIGPIPE back to the default.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
    let matches = Cli::command().get_matches();
    let mut cli = Cli::from_arg_matches(&matches).unwrap_or_else(|e| e.exit());
    cli.profile_flag = profile_flag(&matches);
    match run(&cli) {
        Ok(0) => {}
        Ok(code) => std::process::exit(code),
        Err(e) => std::process::exit(output::fail(&e, cli.common.json)),
    }
}

/// Whether `--profile` was given on the command line, before or after the
/// subcommand, rather than through [`PROFILE_ENV`].
pub fn profile_flag(matches: &ArgMatches) -> bool {
    let mut m = matches;
    let mut on_line = m.value_source("profile") == Some(ValueSource::CommandLine);
    while let Some((_, sub)) = m.subcommand() {
        m = sub;
        on_line |= m.value_source("profile") == Some(ValueSource::CommandLine);
    }
    on_line
}

/// The profile a command signs in as, which must be configured: a missing,
/// malformed or unknown name is exit 2 listing the configured ones, and a
/// lone profile is never picked.
pub fn resolve_profile(given: Option<&str>, configured: &Profiles) -> Result<ProfileName, CliError> {
    profile::resolve(given.filter(|g| !g.is_empty()), PROFILE_ENV, configured, BIN)
}

/// For a command that needs no sign-in: `--profile` on its command line is
/// exit 2, and the variable is not read.
fn no_profile(cli: &Cli, command: &str) -> Result<(), CliError> {
    if cli.profile_flag {
        return Err(CliError::Usage(format!(
            "`{BIN} {command}` needs no sign-in and takes no --profile"
        )));
    }
    Ok(())
}

fn store(cli: &Cli) -> ConfigStore {
    ConfigStore::new(BIN).with_override(cli.config.clone())
}

fn note(cli: &Cli, msg: &str) {
    if !cli.common.quiet {
        eprintln!("{msg}");
    }
}

/// A `YYYY-MM-DD` date, zero-padded, so it compares as a string.
pub fn iso(d: &str) -> Result<String, CliError> {
    Ok(dates::fmt_iso(dates::parse_iso(d.trim())?))
}

fn range(r: &pk_cli_core::RangeArgs) -> Result<(Option<String>, Option<String>), CliError> {
    let since = r.since.as_deref().map(iso).transpose()?;
    let until = r.until.as_deref().map(iso).transpose()?;
    if let (Some(s), Some(u)) = (&since, &until) {
        if s > u {
            return Err(CliError::Usage("--since is after --until".into()));
        }
    }
    Ok((since, until))
}

/// The process's exit code: 0, or an incomplete stream's error code.
pub fn run(cli: &Cli) -> Result<i32, CliError> {
    let profiles = || -> Result<Profiles, CliError> { Ok(store(cli).load::<Config>()?.profiles) };
    let signed_in = || -> Result<ProfileName, CliError> { resolve_profile(cli.profile.as_deref(), &profiles()?) };
    match &cli.command {
        Command::Auth(AuthCmd::Reset { force }) => {
            no_profile(cli, "auth reset")?;
            crate::auth::reset(*force, cli.common.interactive(), cli.common.json, cli.common.quiet)?;
        }
        Command::Auth(cmd) => {
            let profile = signed_in()?;
            let (json, quiet) = (cli.common.json, cli.common.quiet);
            match cmd {
                AuthCmd::Login(args) => crate::auth::login(args, &profile, json, quiet)?,
                AuthCmd::Status => crate::auth::status(&profile, json, quiet)?,
                AuthCmd::Logout => crate::auth::logout(&profile, json, quiet)?,
                AuthCmd::Reset { .. } => unreachable!("handled above"),
            }
        }
        Command::Profiles(cmd) => {
            no_profile(cli, "profiles")?;
            profiles_cmd(cli, cmd)?;
        }
        Command::Config(cmd) => {
            no_profile(cli, "config")?;
            config(cli, cmd)?;
        }
        Command::Records(RecordsCmd::Export(args)) => {
            // Its own checks first: they reach neither the vault nor the site.
            export_args(cli, args)?;
            let profile = signed_in()?;
            return export(cli, &Env::for_binary()?, &profile, args);
        }
        Command::Patients(PatientsCmd::List) => {
            let profile = signed_in()?;
            let env = Env::for_binary()?;
            let site = Site::open(&env, &profile)?;
            let v = portal::patients(&site)?;
            site.finish_quietly();
            print_patients(cli, &v);
        }
        Command::Donations(DonationsCmd::List(r)) => {
            let (since, until) = range(r)?;
            let profile = signed_in()?;
            let env = Env::for_binary()?;
            let site = Site::open(&env, &profile)?;
            let v = portal::donations_list(&site, since.as_deref(), until.as_deref())?;
            site.finish_quietly();
            print_list(cli, &v, portal::DONATION_COLUMNS, "donations");
        }
        Command::Readings(ReadingsCmd::List(args)) => {
            let (since, until) = range(&args.range)?;
            let tests = tests_of(args.test);
            let profile = signed_in()?;
            let env = Env::for_binary()?;
            let site = Site::open(&env, &profile)?;
            let v = portal::readings_list(&site, &tests, since.as_deref(), until.as_deref())?;
            site.finish_quietly();
            print_list(cli, &v, portal::READING_COLUMNS, "readings");
        }
        Command::Api(args) => {
            let path = crate::client::api_path(&args.path)?;
            let profile = signed_in()?;
            let env = Env::for_binary()?;
            let site = Site::open(&env, &profile)?;
            let v = site.api(&path)?;
            site.finish_quietly();
            output::json(&v);
        }
        Command::SelfUpdate(args) => {
            no_profile(cli, "self-update")?;
            Updater {
                repo: REPO.into(),
                binary: BIN.into(),
                target: env!("BUILD_TARGET").into(),
                current: env!("CARGO_PKG_VERSION").into(),
            }
            .run(args, cli.common.json, cli.common.quiet)?;
        }
        Command::Completions { shell } => {
            no_profile(cli, "completions")?;
            clap_complete::generate(*shell, &mut Cli::command(), BIN, &mut std::io::stdout());
        }
        Command::Info => {
            no_profile(cli, "info")?;
            output::json(&serde_json::to_value(info()).expect("CliInfo serializes"));
        }
    }
    Ok(0)
}

/// `info --json` (cli-info/v1).
pub fn info() -> CliInfo {
    CliInfo::new(
        BIN,
        env!("CARGO_PKG_VERSION"),
        &format!("https://github.com/{REPO}"),
        AuthInfo {
            required: true,
            method: "browser-session".into(),
            login_hint: Some(format!("{BIN} auth login --browser --profile <NAME>")),
        },
        &[
            "profiles",
            "patients",
            "records",
            "donations",
            "readings",
            "api",
            // Every login command, records export among them, takes --profile.
            pk_med_records::LOGIN_PROFILES,
        ],
    )
    .with_profiles(PROFILES)
}

/// The tests `--test` names: blood pressure is two.
pub fn tests_of(arg: Option<TestArg>) -> Vec<Test> {
    match arg {
        None => Test::ALL.to_vec(),
        Some(TestArg::Cholesterol) => vec![Test::Cholesterol],
        Some(TestArg::BloodPressure) => vec![Test::Systolic, Test::Diastolic],
        Some(TestArg::Pulse) => vec![Test::Pulse],
        Some(TestArg::Temperature) => vec![Test::Temperature],
        Some(TestArg::Hemoglobin) => vec![Test::Hemoglobin],
    }
}

fn print_patients(cli: &Cli, v: &serde_json::Value) {
    if cli.common.json {
        output::json(v);
    } else {
        output::table(&output::table_view(
            &output::rows_of(v, "items"),
            &["id", "name", "blood_type", "relationship"],
        ));
    }
}

fn print_list(cli: &Cli, v: &serde_json::Value, columns: &[&str], what: &str) {
    if cli.common.json {
        output::json(v);
        return;
    }
    let rows = output::rows_of(v, "items");
    if rows.is_empty() {
        note(cli, &format!("no {what}"));
    } else {
        output::table(&output::table_view(&rows, columns));
    }
    if let Some(u) = v.get("unreadable").and_then(|u| u.as_object()) {
        let parts: Vec<String> = u.iter().map(|(k, n)| format!("{k} {n}")).collect();
        note(
            cli,
            &format!("note: left out what came without a date or value: {}", parts.join(", ")),
        );
    }
}

// ---------------------------------------------------------------- export

/// `records export`'s own checks, before any profile, vault or request.
pub fn export_args(cli: &Cli, args: &ExportArgs) -> Result<(), CliError> {
    if !args.ndjson && args.output.is_none() {
        return Err(CliError::Usage(
            "say where the stream goes: --ndjson for stdout (`| medrec import --from oneblood`), or -o <DIR>".into(),
        ));
    }
    if args.ndjson && cli.common.json {
        return Err(CliError::Usage(
            "--ndjson writes the records stream, which is the output; drop --json".into(),
        ));
    }
    if let Some(s) = &args.since {
        iso(s)?;
    }
    Ok(())
}

/// The exit code of a stream that ended incomplete: its error's.
pub fn incomplete_exit(trailer: &Trailer) -> i32 {
    trailer.error.as_ref().and_then(|e| e.exit_code).unwrap_or(1)
}

pub fn export(cli: &Cli, env: &Env, profile: &ProfileName, args: &ExportArgs) -> Result<i32, CliError> {
    let since = args.since.as_deref().map(iso).transpose()?;
    if let Some(dir) = &args.output {
        // A plain file of records is for a test's server only: real records
        // go straight into medrec through a pipe.
        if env.is_real_site() {
            return Err(CliError::Usage(
                "-o writes records to a plain file, which is for tests against a local server only".into(),
            )
            .with_remedy(format!(
                "pipe real records into medrec: `{BIN} records export --profile {profile} --ndjson | medrec import \
                 --from oneblood --from-profile {profile} --patient <ID>`"
            )));
        }
        let site = Site::open(env, profile)?;
        let (path, finished) = export_to_dir(dir, &site, args.patient.as_deref(), since.as_deref())?;
        site.finish_quietly();
        let summary = RecordsExport::new(path.display().to_string(), &finished);
        if cli.common.json {
            output::json(&serde_json::to_value(&summary).unwrap_or_default());
        } else {
            note(
                cli,
                &format!("wrote {} ({} resources)", path.display(), finished.trailer.total()),
            );
        }
        return Ok(if finished.trailer.complete {
            0
        } else {
            incomplete_exit(&finished.trailer)
        });
    }
    let site = Site::open(env, profile)?;
    let stdout = std::io::stdout();
    let finished = portal::export(&site, stdout.lock(), args.patient.as_deref(), since.as_deref());
    match finished {
        Ok(f) if f.trailer.complete => {
            site.finish_quietly();
            Ok(0)
        }
        // The trailer carries the error; exit without a second document.
        Ok(f) => Ok(incomplete_exit(&f.trailer)),
        Err(e) => Err(e),
    }
}

fn export_to_dir(
    dir: &Path,
    site: &Site,
    patient: Option<&str>,
    since: Option<&str>,
) -> Result<(PathBuf, Finished), CliError> {
    let mut export = create_export_file(dir)?;
    let finished = portal::export(site, std::io::BufWriter::new(&mut export), patient, since)?;
    let path = export.commit(&finished)?;
    Ok((path, finished))
}

// -------------------------------------------------------------- profiles

/// `profiles list|add|remove` (cli-common DESIGN.md §2.5): the logins, by
/// name, in the config. Removing one removes what oneblood keeps for it
/// first.
fn profiles_cmd(cli: &Cli, cmd: &ProfilesCmd) -> Result<(), CliError> {
    let store = store(cli);
    let json_mode = cli.common.json;
    match cmd {
        ProfilesCmd::List => {
            let cfg: Config = store.load()?;
            let list = cfg.profiles.list();
            if json_mode {
                output::json(&serde_json::to_value(&list).unwrap_or_default());
            } else if list.profiles.is_empty() {
                note(
                    cli,
                    &format!("no profiles: `{BIN} profiles add <NAME>`, one per donor-portal account"),
                );
            } else {
                for p in &list.profiles {
                    println!("{}", p.name);
                }
            }
            Ok(())
        }
        ProfilesCmd::Add { name } => {
            let name = ProfileName::parse(name)?;
            let mut cfg: Config = store.load()?;
            let added = cfg.profiles.add(name.clone());
            if added {
                store.save(&cfg)?;
            }
            output::emit(
                json_mode,
                "profile-add",
                json!({ "profile": name.as_str(), "added": added }),
                |_| {
                    note(
                        cli,
                        &if added {
                            format!("added profile {name}; sign in with `{BIN} auth login --browser --profile {name}`")
                        } else {
                            format!("profile {name} is already configured")
                        },
                    )
                },
            );
            Ok(())
        }
        ProfilesCmd::Remove { name, force } => {
            let name = ProfileName::parse(name)?;
            // Whether it can ask, before the config or the vault is read.
            pk_cli_core::confirm::require_confirmable(*force, cli.common.interactive(), "remove a profile")?;
            let cfg: Config = store.load()?;
            if !cfg.profiles.contains(&name) {
                return Err(CliError::NotFound(format!("no profile `{name}`"))
                    .with_remedy(format!("`{BIN} profiles list` shows the configured ones")));
            }
            pk_cli_core::confirm::confirm(
                *force,
                &format!("Remove profile {name}, its stored session and its browser profile?"),
            )?;
            let done = crate::auth::forget_login(&name)?;
            let mut cfg: Config = store.load()?;
            cfg.profiles.remove(&name);
            store.save(&cfg)?;
            let mut v = json!({ "profile": name.as_str(), "session_removed": done.session_removed });
            if let Some(removed) = done.browser_profile_removed {
                v["browser_profile_removed"] = removed.into();
            }
            output::emit(json_mode, "profile-remove", v, |_| {
                note(
                    cli,
                    &format!(
                        "removed profile {name}: {}",
                        if done.session_removed {
                            "its session was removed"
                        } else {
                            "no session was stored"
                        }
                    ),
                )
            });
            Ok(())
        }
    }
}

// ---------------------------------------------------------------- config

fn config(cli: &Cli, cmd: &ConfigCmd) -> Result<(), CliError> {
    let store = store(cli);
    match cmd {
        ConfigCmd::Path => {
            let path = store.path()?;
            if cli.common.json {
                output::json(&json!({ "schema": "config-path/v1", "path": path.display().to_string() }));
            } else {
                println!("{}", path.display());
            }
        }
        ConfigCmd::Show => {
            let cfg: Config = store.load()?;
            let mut v = serde_json::to_value(&cfg).unwrap_or_default();
            if cli.common.json {
                v["schema"] = "config/v1".into();
                output::json(&v);
            } else if v.as_object().is_some_and(|o| o.is_empty()) {
                note(
                    cli,
                    &format!("no settings; profiles are added with `{BIN} profiles add <NAME>`"),
                );
            } else {
                output::render(&v);
            }
        }
    }
    Ok(())
}
