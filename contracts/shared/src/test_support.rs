//! Scoped-authorization helpers for Soroban tests (issue #314).
//!
//! `Env::mock_all_auths()` authorizes *every* address in *every* invocation
//! tree, so a test written that way keeps passing even after a contract drops
//! its `require_auth` call entirely. That is the failure mode these helpers
//! exist to prevent: an authorization test must fail if the guard is removed,
//! and a behavior test should only authorize the calls it actually intends to
//! allow.
//!
//! Two building blocks are provided:
//!
//! * [`allow`] / [`allow_with_sub_invocations`] install an exact `MockAuth`
//!   tree, so any `require_auth` not described reverts with an auth error.
//! * [`assert_authorized`] / [`assert_unauthorized`] inspect `Env::auths()` to
//!   prove *which* address authorized *which* call. This catches the inverse
//!   bug, where an over-broad tree authorizes more than the contract needs.
//!
//! # Which sub-invocations need an entry
//!
//! Every `require_auth` that the host must match appears in the `MockAuth`
//! tree, including nested ones. `Pair::mint_with_one_token`, for example, pulls
//! the deposit with a token `transfer` that the payer must separately
//! authorize, so a faithful tree contains that `transfer` as a sub-invocation
//! of `mint_with_one_token`.
//!
//! There is one important exception, and it is a limitation of the *mock*, not
//! of Soroban: `Env::mock_auths` installs a `MockAuthContract` at each
//! authorizer address. If a contract you need to authorize is already
//! registered at that address, the mock replaces it and the host no longer
//! executes the real contract on that sub-invocation.
//!
//! In CoralSwap that rules out building a scoped tree for the pair acting as
//! its own LP token's `admin`, and for a deployed pair acting as its own
//! governance signer. Tests covering those paths use
//! `Env::mock_all_auths()` together with [`assert_authorized`], which still
//! proves that the *right* address authorized the call and catches a removed
//! guard. The pure rejection cases — wrong signer, wrong function, wrong
//! arguments, no authorization at all — need no such workaround and are
//! asserted with a scoped tree or [`allow_nothing`].
//!
//! # Argument binding
//!
//! A bare `require_auth()` is argument-bound: the SDK infers the current
//! invocation's arguments, exactly as `require_auth_for_args(..)` would. An
//! authorization for `amount = 1_000` therefore does not satisfy a call with
//! `amount = 1`, and the `auth_matrix` tests assert this directly.
//!
//! # Example
//!
//! ```ignore
//! use coralswap_shared::test_support as auth;
//!
//! auth::allow(&env, &user, &pair_id, "burn", auth::args(&env, &[&user]));
//! pair_client.burn(&user);
//! auth::assert_authorized(&env, &user, &pair_id, "burn", auth::args(&env, &[&user]));
//! ```

extern crate alloc;

use alloc::boxed::Box;
use alloc::string::{String, ToString};
pub use alloc::vec::Vec;
pub use soroban_sdk::Vec as SorobanVec;

use soroban_sdk::testutils::{AuthorizedFunction, AuthorizedInvocation};
use soroban_sdk::testutils::{MockAuth, MockAuthInvoke};
use soroban_sdk::Vec as SdkVec;
use soroban_sdk::{Address, Env, IntoVal, Symbol, Val};

/// A node in an authorization tree, described with owned fields so tests can
/// build trees in a plain `vec![...]` literal.
///
/// Used for both root invocations and nested ones.
#[derive(Clone, Debug)]
pub struct Call {
    pub contract: Address,
    pub fn_name: String,
    pub args: SdkVec<Val>,
    pub sub_invocations: Vec<Call>,
}

impl Call {
    /// A leaf call: `contract::fn_name(args)`.
    pub fn new(contract: &Address, fn_name: &str, args: SdkVec<Val>) -> Self {
        Call {
            contract: contract.clone(),
            fn_name: String::from(fn_name),
            args,
            sub_invocations: Vec::new(),
        }
    }

    /// Adds nested authorized calls beneath this one.
    pub fn with_sub_invocations(mut self, sub_invocations: Vec<Call>) -> Self {
        self.sub_invocations = sub_invocations;
        self
    }
}

/// Backwards-friendly alias for the leaf-only use case.
pub type SubCall = Call;

/// Converts a single value into a [`Val`]. Used by the [`auth_args!`] macro.
pub fn to_val<T: IntoVal<Env, Val>>(env: &Env, value: T) -> Val {
    value.into_val(env)
}

/// Builds a `Vec<Val>` of invocation arguments for a [`MockAuth`], for the
/// common case where every element shares one type.
/// An empty argument list, for zero-argument entry points such as `pause`.
pub fn no_args(env: &Env) -> SdkVec<Val> {
    SdkVec::new(env)
}

pub fn args<T: IntoVal<Env, Val>>(env: &Env, values: &[T]) -> SdkVec<Val> {
    let mut out = SdkVec::new(env);
    for value in values {
        out.push_back(value.into_val(env));
    }
    out
}

/// Builds a `Vec<Val>` of invocation arguments, allowing each argument to have
/// a different type.
///
/// A macro rather than a function because Rust array and slice literals are
/// homogeneous, while a `MockAuth` argument list is not — it routinely mixes
/// addresses, `i128`s, and `u32`s.
///
/// ```
/// use coralswap_shared::test_support::auth_args;
/// # use soroban_sdk::Env;
/// # let env = Env::default();
/// # let user = soroban_sdk::testutils::Address::generate(&env);
/// # let token = soroban_sdk::testutils::Address::generate(&env);
/// let args = auth_args!(&env, user.clone(), token, 1_000i128, 7u32);
/// assert_eq!(args.len(), 4);
/// ```
#[macro_export]
macro_rules! auth_args {
    ($env:expr, $($value:expr),* $(,)?) => {{
        let env = $env;
        let mut args = $crate::test_support::SorobanVec::new(env);
        $( args.push_back($crate::test_support::to_val(env, $value)); )*
        args
    }};
}

/// Recursively lowers a [`Call`] tree into the borrowed form the SDK wants.
///
/// `MockAuthInvoke` borrows its contract address, function name, and children,
/// which makes a self-referential structure impossible to build on the stack
/// for arbitrary depth. Leaking the small per-call allocations is the simplest
/// correct option for a test-only helper: a test authorizes a handful of calls
/// and the process exits afterwards.
fn build_invoke(call: &Call) -> MockAuthInvoke<'static> {
    let contract: &'static Address = Box::leak(Box::new(call.contract.clone()));
    let fn_name: &'static str = Box::leak(call.fn_name.clone().into_boxed_str());
    let sub_invokes: &'static [MockAuthInvoke<'static>] = if call.sub_invocations.is_empty() {
        &[]
    } else {
        let children: Vec<MockAuthInvoke<'static>> =
            call.sub_invocations.iter().map(build_invoke).collect();
        Box::leak(children.into_boxed_slice())
    };
    MockAuthInvoke { contract, fn_name, args: call.args.clone(), sub_invokes }
}

/// Authorizes `address` for exactly one call to `contract::fn_name(args)`.
///
/// Any other `require_auth` in the invocation tree fails the test, so this
/// doubles as an assertion that the contract does not over-authorize.
pub fn allow(env: &Env, address: &Address, contract: &Address, fn_name: &str, args: SdkVec<Val>) {
    allow_with_sub_invocations(env, address, contract, fn_name, args, &[]);
}

/// Authorizes `address` for one call to `contract::fn_name(args)` that is
/// expected to make further authorized sub-invocations.
///
/// This is the realistic reentrancy shape: `Pair::mint_with_one_token` pulls
/// the deposit with a token `transfer` the payer must authorize, and
/// `Pair::burn_single_side` burns LP that the pair custodies on the holder's
/// behalf.
pub fn allow_with_sub_invocations(
    env: &Env,
    address: &Address,
    contract: &Address,
    fn_name: &str,
    args: SdkVec<Val>,
    sub_invocations: &[Call],
) {
    let nested: Vec<MockAuthInvoke> = sub_invocations.iter().map(build_invoke).collect();
    let root = MockAuthInvoke { contract, fn_name, args, sub_invokes: &nested };
    env.mock_auths(&[MockAuth { address, invoke: &root }]);
}

/// Authorizes `address` for several root calls, with no nesting.
///
/// `Env::mock_auths` replaces the whole tree on every call, so a fixture that
/// needs two authorizations for the same address must install them together.
pub fn allow_calls(env: &Env, address: &Address, calls: &[Call]) {
    let roots: Vec<MockAuthInvoke> = calls.iter().map(build_invoke).collect();
    let auths: Vec<MockAuth> =
        roots.iter().map(|root| MockAuth { address, invoke: root }).collect();
    env.mock_auths(&auths);
}

/// Authorizes nothing at all.
///
/// For negative tests that assert an unauthorized caller is rejected: the
/// absence of a tree is the whole point.
pub fn allow_nothing(env: &Env) {
    env.mock_auths(&[]);
}

/// Asserts the most recent invocation was authorized by exactly `address`, for
/// exactly `contract::fn_name(args)`, with no authorized sub-invocations.
///
/// This is the check a blanket mock cannot provide: it proves the guard exists
/// rather than merely proving the happy path runs.
pub fn assert_authorized(
    env: &Env,
    address: &Address,
    contract: &Address,
    fn_name: &str,
    args: SdkVec<Val>,
) {
    let auths = env.auths();
    assert_eq!(auths.len(), 1, "expected exactly one authorized actor, got {}", describe(env));
    assert_eq!(auths[0].0, *address, "wrong address authorized the call");
    let expected = AuthorizedInvocation {
        function: AuthorizedFunction::Contract((contract.clone(), Symbol::new(env, fn_name), args)),
        sub_invocations: Vec::new(),
    };
    assert_eq!(auths[0].1, expected, "authorized call did not match");
}

/// Asserts the most recent invocation performed no authorization at all.
///
/// For entry points that are intentionally permissionless (`swap`, `sync`,
/// `flash_loan`), this pins the intent down so a future `require_auth` becomes
/// a deliberate, reviewable change instead of an accident.
pub fn assert_unauthorized(env: &Env) {
    assert!(env.auths().is_empty(), "expected no authorized actors, got {}", describe(env));
}

/// Renders the recorded authorization tree for readable assertion failures.
pub fn describe(env: &Env) -> String {
    let mut out = String::from("[");
    for inv in env.auths().into_iter().map(|(_, i)| i) {
        out.push_str("calls: ");
        out.push_str(&render(&inv, 1));
        out.push_str("; ");
    }
    out.push(']');
    out
}

fn render(inv: &AuthorizedInvocation, depth: usize) -> String {
    let mut out = String::new();
    for _ in 0..depth {
        out.push_str("  ");
    }
    match &inv.function {
        AuthorizedFunction::Contract((_contract, fn_name, _args)) => {
            out.push_str(&fn_name.to_string());
        }
        other => {
            let _ = other;
            out.push_str("<non-contract function>");
        }
    }
    for sub in &inv.sub_invocations {
        out.push('\n');
        out.push_str(&render(sub, depth + 1));
    }
    out
}
