//! The command tree (piekstra-cli/1, health-records/v1).

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};
use clap_complete::Shell;
use pk_cli_auth::LoginArgs;
use pk_cli_browser::BrowserArgs;
use pk_cli_core::{CommonArgs, RangeArgs};
use pk_cli_selfupdate::SelfUpdateArgs;

pub const BIN: &str = "oneblood";

/// A OneBlood blood donor's blood type and the readings taken at each
/// donation (cholesterol, blood pressure, pulse, temperature, hemoglobin),
/// read from the donor portal on oneblood.org. Read-only; unofficial.
#[derive(Parser, Debug)]
#[command(name = BIN, version, about, long_about = None)]
pub struct Cli {
    #[command(flatten)]
    pub common: CommonArgs,

    /// Override the config file location.
    #[arg(long, global = true, value_name = "PATH")]
    pub config: Option<PathBuf>,

    /// Read the donor portal as this profile: one configured with `oneblood
    /// profiles add <NAME>` and signed in with `oneblood auth login --browser
    /// --profile <NAME>`. There is no default profile.
    #[arg(long, global = true, env = "ONEBLOOD_PROFILE", value_name = "NAME")]
    pub profile: Option<String>,

    /// Whether --profile was given as a flag, not through ONEBLOOD_PROFILE.
    #[arg(skip)]
    pub profile_flag: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// The donor-portal sign-in, one per --profile.
    #[command(subcommand)]
    Auth(AuthCmd),
    /// The donor-portal logins: one profile per account (profile-list/v1).
    #[command(subcommand)]
    Profiles(ProfilesCmd),
    /// The donor the profile signs in as (patient-list/v1).
    #[command(subcommand)]
    Patients(PatientsCmd),
    /// Blood type and every reading as FHIR Observations in
    /// records-stream/v1, for `medrec import --from oneblood`.
    #[command(subcommand)]
    Records(RecordsCmd),
    /// Donations, newest first, with the readings taken at each.
    #[command(subcommand)]
    Donations(DonationsCmd),
    /// The readings, one row per test and donation.
    #[command(subcommand)]
    Readings(ReadingsCmd),
    /// One read of the donor portal as --profile <NAME>, printed as the JSON
    /// it answers: `GET <PATH>` under /api/invita/. Read-only: the sign-in
    /// paths and any query naming an action are refused before a request is
    /// sent.
    Api(ApiArgs),
    /// Non-secret settings: the config file and the profiles in it.
    #[command(subcommand)]
    Config(ConfigCmd),
    /// Update to the latest release from GitHub.
    SelfUpdate(SelfUpdateArgs),
    /// Print a shell completion script.
    Completions { shell: Shell },
    /// Machine-readable capability discovery (cli-info/v1).
    Info,
}

/// `oneblood api GET <PATH>`: cli-common's `ApiArgs` offers writes, and
/// oneblood only reads.
#[derive(Args, Debug, Clone)]
pub struct ApiArgs {
    /// GET (any case): the only method sent.
    #[arg(value_enum, ignore_case = true)]
    pub method: ApiMethod,
    /// A path on https://www.oneblood.org (leading /), or a URL on it.
    pub path: String,
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApiMethod {
    #[value(name = "GET")]
    Get,
}

#[derive(Subcommand, Debug)]
pub enum AuthCmd {
    /// Sign in to the donor portal as --profile <NAME> and keep the session
    /// in oneblood's vault: encrypted files under ~/.config/oneblood/vault/,
    /// sealed with a key held in the OS keychain (auth-login/v1).
    ///
    /// --browser opens a Chrome window on www.oneblood.org in the profile's
    /// own browser profile, where the donor signs in (email address,
    /// password and the code OneBlood sends); --stdin reads the Cookie header
    /// of a www.oneblood.org request, copied from a signed-in browser's
    /// DevTools, with --user-agent from the same request. One read checks the
    /// session before it is stored, unless --no-verify.
    Login(LoginCmd),
    /// Whether the profile's session is stored and when it was captured
    /// (auth-status/v1). Sends no request.
    Status,
    /// Remove the profile's stored session, then the browser profile its
    /// --browser sign-in keeps (auth-logout/v1).
    Logout,
    /// Remove every profile's stored session from the vault and the vault's
    /// key from the keychain (auth-reset/v1): the way out when the key is
    /// lost or damaged. Asks first; without a terminal it needs --force
    /// (exit 6).
    Reset {
        /// Do not ask.
        #[arg(long)]
        force: bool,
    },
}

// The family's login flags with `--browser` beside them (DESIGN.md §2.2).
// A plain comment: a doc comment here would replace `auth login`'s about.
#[derive(Args, Debug)]
pub struct LoginCmd {
    #[command(flatten)]
    pub login: LoginArgs,
    #[command(flatten)]
    pub browser: BrowserArgs,
    /// With --stdin or --from-env: the User-Agent of the browser the Cookie
    /// header was copied from, sent with the check and every later read.
    #[arg(long, value_name = "UA")]
    pub user_agent: Option<String>,
}

#[derive(Subcommand, Debug)]
pub enum ProfilesCmd {
    /// The configured profiles, by name (profile-list/v1).
    #[command(visible_alias = "ls")]
    List,
    /// Add a profile: 1 to 32 lowercase letters, digits, `-` or `_`,
    /// starting with a letter or a digit (profile-add/v1).
    Add { name: String },
    /// Remove a profile with its stored session and the browser profile its
    /// --browser sign-in keeps (profile-remove/v1). Asks first; without a
    /// terminal it needs --force (exit 6).
    Remove {
        name: String,
        /// Do not ask.
        #[arg(long)]
        force: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum ConfigCmd {
    /// Print the config file path.
    Path,
    /// Show the effective configuration.
    Show,
}

#[derive(Subcommand, Debug)]
pub enum PatientsCmd {
    /// The donor the profile signs in as: one per login (patient-list/v1).
    #[command(visible_alias = "ls")]
    List,
}

#[derive(Subcommand, Debug)]
pub enum RecordsCmd {
    /// The donor's blood type and readings as records-stream/v1.
    Export(ExportArgs),
}

#[derive(Args, Debug)]
pub struct ExportArgs {
    /// The donor: the id from `oneblood patients list`. May be omitted: a
    /// login is one donor.
    #[arg(long, value_name = "ID")]
    pub patient: Option<String>,
    /// Only readings taken on or after this day (YYYY-MM-DD). The blood type
    /// is always included.
    #[arg(long, value_name = "YYYY-MM-DD")]
    pub since: Option<String>,
    /// Write the stream to stdout (`| medrec import --from oneblood`). The
    /// stream is the output, so --json with it is a usage error.
    #[arg(long, conflicts_with = "output")]
    pub ndjson: bool,
    /// Write the stream to DIR/records.ndjson instead (tests and synthetic
    /// data only: real records go straight into medrec).
    #[arg(short, long, value_name = "DIR")]
    pub output: Option<PathBuf>,
}

#[derive(Subcommand, Debug)]
pub enum DonationsCmd {
    /// Donations, newest first, each with its readings (donation-list/v1).
    #[command(visible_alias = "ls")]
    List(RangeArgs),
}

#[derive(Subcommand, Debug)]
pub enum ReadingsCmd {
    /// One row per reading, newest first (reading-list/v1).
    #[command(visible_alias = "ls")]
    List(ReadingsArgs),
}

#[derive(Args, Debug)]
pub struct ReadingsArgs {
    /// Only this test.
    #[arg(long, value_enum)]
    pub test: Option<TestArg>,
    #[command(flatten)]
    pub range: RangeArgs,
}

/// The tests the donor portal charts.
#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum TestArg {
    Cholesterol,
    BloodPressure,
    Pulse,
    Temperature,
    /// The portal's "iron count".
    #[value(alias = "iron")]
    Hemoglobin,
}
