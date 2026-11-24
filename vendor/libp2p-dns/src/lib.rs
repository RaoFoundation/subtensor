// Copyright 2018 Parity Technologies (UK) Ltd.
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the "Software"),
// to deal in the Software without restriction, including without limitation
// the rights to use, copy, modify, merge, publish, distribute, sublicense,
// and/or sell copies of the Software, and to permit persons to whom the
// Software is furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
// FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
// DEALINGS IN THE SOFTWARE.

//! # [DNS name resolution](https://github.com/libp2p/specs/blob/master/addressing/README.md#ip-and-name-resolution)
//! [`Transport`] for libp2p.
//!
//! This crate provides the type [`async_std::Transport`] and [`tokio::Transport`]
//! for use with `async-std` and `tokio`,
//! respectively.
//!
//! A [`Transport`] is an address-rewriting [`libp2p_core::Transport`] wrapper around
//! an inner `Transport`. The composed transport behaves like the inner
//! transport, except that [`libp2p_core::Transport::dial`] resolves `/dns/...`, `/dns4/...`,
//! `/dns6/...` and `/dnsaddr/...` components of the given `Multiaddr` through
//! a DNS, replacing them with the resolved protocols (typically TCP/IP).
//!
//! The `async-std` feature and hence the [`async_std::Transport`] are
//! enabled by default. Tokio users can furthermore opt-in
//! to the `tokio-dns-over-rustls` and `tokio-dns-over-https-rustls`
//! features. For more information about these features, please
//! refer to the documentation of [trust-dns-resolver].
//!
//! On Unix systems, if no custom configuration is given, [trust-dns-resolver]
//! will try to parse the `/etc/resolv.conf` file. This approach comes with a
//! few caveats to be aware of:
//!   1) This fails (panics even!) if `/etc/resolv.conf` does not exist. This is
//!      the case on all versions of Android.
//!   2) DNS configuration is only evaluated during startup. Runtime changes are
//!      thus ignored.
//!   3) DNS resolution is obviously done in process and consequently not using
//!      any system APIs (like libc's `gethostbyname`). Again this is
//!      problematic on platforms like Android, where there's a lot of
//!      complexity hidden behind the system APIs.
//!
//! If the implementation requires different characteristics, one should
//! consider providing their own implementation of [`Transport`] or use
//! platform specific APIs to extract the host's DNS configuration (if possible)
//! and provide a custom [`ResolverConfig`].
//!
//![trust-dns-resolver]: https://docs.rs/trust-dns-resolver/latest/trust_dns_resolver/#dns-over-tls-and-dns-over-https

#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]

#[cfg(feature = "tokio")]
pub mod tokio {
    use parking_lot::Mutex;
    use std::sync::Arc;

    use super::{ResolverConfig, ResolverOpts, SystemResolver};

    /// A `Transport` wrapper for performing DNS lookups when dialing `Multiaddr`esses
    /// using `tokio` for all async I/O.
    pub type Transport<T> = crate::Transport<T, SystemResolver>;

    impl<T> Transport<T> {
        /// Creates a new [`Transport`] from the OS's DNS configuration and defaults.
        pub fn system(inner: T) -> Result<Transport<T>, std::io::Error> {
            Ok(Self::custom(inner, ResolverConfig::default(), ResolverOpts::default()))
        }

        /// Creates a [`Transport`] with a custom resolver configuration
        /// and options. Config/opts are accepted for API compatibility and ignored;
        /// resolution uses the OS resolver via `::tokio::net::lookup_host`.
        pub fn custom(
            inner: T,
            _cfg: ResolverConfig,
            _opts: ResolverOpts,
        ) -> Transport<T> {
            Transport {
                inner: Arc::new(Mutex::new(inner)),
                resolver: SystemResolver,
            }
        }
    }
}

use async_trait::async_trait;
use futures::{future::BoxFuture, prelude::*};
use libp2p_core::{
    multiaddr::{Multiaddr, Protocol},
    transport::{DialOpts, ListenerId, TransportError, TransportEvent},
};
use parking_lot::Mutex;
use smallvec::SmallVec;
use std::io;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::{
    error, fmt, iter,
    ops::DerefMut,
    pin::Pin,
    str,
    sync::Arc,
    task::{Context, Poll},
};

/// Stub DNS config retained for `libp2p` builder API compatibility.
#[derive(Debug, Clone, Default)]
pub struct ResolverConfig;

/// Stub DNS options retained for `libp2p` builder API compatibility.
#[derive(Debug, Clone, Default)]
pub struct ResolverOpts;

/// DNS resolution error (OS resolver).
#[derive(Debug)]
pub struct ResolveError(std::io::Error);

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ResolveError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.0)
    }
}

impl From<std::io::Error> for ResolveError {
    fn from(e: std::io::Error) -> Self {
        Self(e)
    }
}

impl From<&'static str> for ResolveError {
    fn from(msg: &'static str) -> Self {
        Self(std::io::Error::new(std::io::ErrorKind::Other, msg))
    }
}

/// Compatibility shim for callers matching on resolution error kinds.
#[derive(Debug)]
pub enum ResolveErrorKind {
    Message(&'static str),
    NoRecordsFound { query: String },
}

impl ResolveError {
    pub fn kind(&self) -> ResolveErrorKind {
        ResolveErrorKind::Message("resolve error")
    }
}

impl From<ResolveErrorKind> for ResolveError {
    fn from(k: ResolveErrorKind) -> Self {
        match k {
            ResolveErrorKind::Message(m) => m.into(),
            ResolveErrorKind::NoRecordsFound { .. } => "No matching records found.".into(),
        }
    }
}

/// The prefix for `dnsaddr` protocol TXT record lookups.
const DNSADDR_PREFIX: &str = "_dnsaddr.";

/// The maximum number of dialing attempts to resolved addresses.
const MAX_DIAL_ATTEMPTS: usize = 16;

/// The maximum number of DNS lookups when dialing.
///
/// This limit is primarily a safeguard against too many, possibly
/// even cyclic, indirections in the addresses obtained from the
/// TXT records of a `/dnsaddr`.
const MAX_DNS_LOOKUPS: usize = 32;

/// The maximum number of TXT records applicable for the address
/// being dialed that are considered for further lookups as a
/// result of a single `/dnsaddr` lookup.
const MAX_TXT_RECORDS: usize = 16;

/// A [`Transport`] for performing DNS lookups when dialing `Multiaddr`esses.
/// You shouldn't need to use this type directly. Use [`tokio::Transport`] or [`async_std::Transport`] instead.
#[derive(Debug)]
pub struct Transport<T, R> {
    /// The underlying transport.
    inner: Arc<Mutex<T>>,
    /// The DNS resolver used when dialing addresses with DNS components.
    resolver: R,
}

impl<T, R> libp2p_core::Transport for Transport<T, R>
where
    T: libp2p_core::Transport + Send + Unpin + 'static,
    T::Error: Send,
    T::Dial: Send,
    R: Clone + Send + Sync + Resolver + 'static,
{
    type Output = T::Output;
    type Error = Error<T::Error>;
    type ListenerUpgrade = future::MapErr<T::ListenerUpgrade, fn(T::Error) -> Self::Error>;
    type Dial = future::Either<
        future::MapErr<T::Dial, fn(T::Error) -> Self::Error>,
        BoxFuture<'static, Result<Self::Output, Self::Error>>,
    >;

    fn listen_on(
        &mut self,
        id: ListenerId,
        addr: Multiaddr,
    ) -> Result<(), TransportError<Self::Error>> {
        self.inner
            .lock()
            .listen_on(id, addr)
            .map_err(|e| e.map(Error::Transport))
    }

    fn remove_listener(&mut self, id: ListenerId) -> bool {
        self.inner.lock().remove_listener(id)
    }

    fn dial(
        &mut self,
        addr: Multiaddr,
        dial_opts: DialOpts,
    ) -> Result<Self::Dial, TransportError<Self::Error>> {
        Ok(self.do_dial(addr, dial_opts))
    }

    fn poll(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<TransportEvent<Self::ListenerUpgrade, Self::Error>> {
        let mut inner = self.inner.lock();
        libp2p_core::Transport::poll(Pin::new(inner.deref_mut()), cx).map(|event| {
            event
                .map_upgrade(|upgr| upgr.map_err::<_, fn(_) -> _>(Error::Transport))
                .map_err(Error::Transport)
        })
    }
}

impl<T, R> Transport<T, R>
where
    T: libp2p_core::Transport + Send + Unpin + 'static,
    T::Error: Send,
    T::Dial: Send,
    R: Clone + Send + Sync + Resolver + 'static,
{
    fn do_dial(
        &mut self,
        addr: Multiaddr,
        dial_opts: DialOpts,
    ) -> <Self as libp2p_core::Transport>::Dial {
        let resolver = self.resolver.clone();
        let inner = self.inner.clone();

        // Asynchronously resolve all DNS names in the address before proceeding
        // with dialing on the underlying transport.
        async move {
            let mut last_err = None;
            let mut dns_lookups = 0;
            let mut dial_attempts = 0;
            // We optimise for the common case of a single DNS component
            // in the address that is resolved with a single lookup.
            let mut unresolved = SmallVec::<[Multiaddr; 1]>::new();
            unresolved.push(addr.clone());

            // Resolve (i.e. replace) all DNS protocol components, initiating
            // dialing attempts as soon as there is another fully resolved
            // address.
            while let Some(addr) = unresolved.pop() {
                if let Some((i, name)) = addr.iter().enumerate().find(|(_, p)| {
                    matches!(
                        p,
                        Protocol::Dns(_)
                            | Protocol::Dns4(_)
                            | Protocol::Dns6(_)
                            | Protocol::Dnsaddr(_)
                    )
                }) {
                    if dns_lookups == MAX_DNS_LOOKUPS {
                        tracing::debug!(address=%addr, "Too many DNS lookups, dropping unresolved address");
                        last_err = Some(Error::TooManyLookups);
                        // There may still be fully resolved addresses in `unresolved`,
                        // so keep going until `unresolved` is empty.
                        continue;
                    }
                    dns_lookups += 1;
                    match resolve(&name, &resolver).await {
                        Err(e) => {
                            if unresolved.is_empty() {
                                return Err(e);
                            }
                            // If there are still unresolved addresses, there is
                            // a chance of success, but we track the last error.
                            last_err = Some(e);
                        }
                        Ok(Resolved::One(ip)) => {
                            tracing::trace!(protocol=%name, resolved=%ip);
                            let addr = addr.replace(i, |_| Some(ip)).expect("`i` is a valid index");
                            unresolved.push(addr);
                        }
                        Ok(Resolved::Many(ips)) => {
                            for ip in ips {
                                tracing::trace!(protocol=%name, resolved=%ip);
                                let addr =
                                    addr.replace(i, |_| Some(ip)).expect("`i` is a valid index");
                                unresolved.push(addr);
                            }
                        }
                        Ok(Resolved::Addrs(addrs)) => {
                            let suffix = addr.iter().skip(i + 1).collect::<Multiaddr>();
                            let prefix = addr.iter().take(i).collect::<Multiaddr>();
                            let mut n = 0;
                            for a in addrs {
                                if a.ends_with(&suffix) {
                                    if n < MAX_TXT_RECORDS {
                                        n += 1;
                                        tracing::trace!(protocol=%name, resolved=%a);
                                        let addr =
                                            prefix.iter().chain(a.iter()).collect::<Multiaddr>();
                                        unresolved.push(addr);
                                    } else {
                                        tracing::debug!(
                                            resolved=%a,
                                            "Too many TXT records, dropping resolved"
                                        );
                                    }
                                }
                            }
                        }
                    }
                } else {
                    // We have a fully resolved address, so try to dial it.
                    tracing::debug!(address=%addr, "Dialing address");

                    let transport = inner.clone();
                    let dial = transport.lock().dial(addr, dial_opts);
                    let result = match dial {
                        Ok(out) => {
                            // We only count attempts that the inner transport
                            // actually accepted, i.e. for which it produced
                            // a dialing future.
                            dial_attempts += 1;
                            out.await.map_err(Error::Transport)
                        }
                        Err(TransportError::MultiaddrNotSupported(a)) => {
                            Err(Error::MultiaddrNotSupported(a))
                        }
                        Err(TransportError::Other(err)) => Err(Error::Transport(err)),
                    };

                    match result {
                        Ok(out) => return Ok(out),
                        Err(err) => {
                            tracing::debug!("Dial error: {:?}.", err);
                            if unresolved.is_empty() {
                                return Err(err);
                            }
                            if dial_attempts == MAX_DIAL_ATTEMPTS {
                                tracing::debug!(
                                    "Aborting dialing after {} attempts.",
                                    MAX_DIAL_ATTEMPTS
                                );
                                return Err(err);
                            }
                            last_err = Some(err);
                        }
                    }
                }
            }

            // At this point, if there was at least one failed dialing
            // attempt, return that error. Otherwise there were no valid DNS records
            // for the given address to begin with (i.e. DNS lookups succeeded but
            // produced no records relevant for the given `addr`).
            Err(last_err.unwrap_or_else(|| {
                Error::ResolveError(ResolveErrorKind::Message("No matching records found.").into())
            }))
        }
        .boxed()
        .right_future()
    }
}

/// The possible errors of a [`Transport`] wrapped transport.
#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum Error<TErr> {
    /// The underlying transport encountered an error.
    Transport(TErr),
    /// DNS resolution failed.
    #[allow(clippy::enum_variant_names)]
    ResolveError(ResolveError),
    /// DNS resolution was successful, but the underlying transport refused the resolved address.
    MultiaddrNotSupported(Multiaddr),
    /// DNS resolution involved too many lookups.
    ///
    /// DNS resolution on dialing performs up to 32 DNS lookups. If these
    /// are not sufficient to obtain a fully-resolved address, this error
    /// is returned and the DNS records for the domain(s) being dialed
    /// should be investigated.
    TooManyLookups,
}

impl<TErr> fmt::Display for Error<TErr>
where
    TErr: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Transport(err) => write!(f, "{err}"),
            Error::ResolveError(err) => write!(f, "{err}"),
            Error::MultiaddrNotSupported(a) => write!(f, "Unsupported resolved address: {a}"),
            Error::TooManyLookups => write!(f, "Too many DNS lookups"),
        }
    }
}

impl<TErr> error::Error for Error<TErr>
where
    TErr: error::Error + 'static,
{
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Error::Transport(err) => Some(err),
            Error::ResolveError(err) => Some(err),
            Error::MultiaddrNotSupported(_) => None,
            Error::TooManyLookups => None,
        }
    }
}

/// The successful outcome of [`resolve`] for a given [`Protocol`].
enum Resolved<'a> {
    /// The given `Protocol` has been resolved to a single `Protocol`,
    /// which may be identical to the one given, in case it is not
    /// a DNS protocol component.
    One(Protocol<'a>),
    /// The given `Protocol` has been resolved to multiple alternative
    /// `Protocol`s as a result of a DNS lookup.
    Many(Vec<Protocol<'a>>),
    /// The given `Protocol` has been resolved to a new list of `Multiaddr`s
    /// obtained from DNS TXT records representing possible alternatives.
    /// These addresses may contain further DNS names that need resolving.
    Addrs(Vec<Multiaddr>),
}

/// Asynchronously resolves the domain name of a `Dns`, `Dns4`, `Dns6` or `Dnsaddr` protocol
/// component. If the given protocol is of a different type, it is returned unchanged as a
/// [`Resolved::One`].
fn resolve<'a, E: 'a + Send, R: Resolver>(
    proto: &Protocol<'a>,
    resolver: &'a R,
) -> BoxFuture<'a, Result<Resolved<'a>, Error<E>>> {
    match proto {
        Protocol::Dns(ref name) => resolver
            .lookup_ip(name.clone().into_owned())
            .map(move |res| match res {
                Ok(ips) => {
                    let mut ips = ips.into_iter();
                    let one = ips
                        .next()
                        .expect("If there are no results, `Err(NoRecordsFound)` is expected.");
                    if let Some(two) = ips.next() {
                        Ok(Resolved::Many(
                            iter::once(one)
                                .chain(iter::once(two))
                                .chain(ips)
                                .map(Protocol::from)
                                .collect(),
                        ))
                    } else {
                        Ok(Resolved::One(Protocol::from(one)))
                    }
                }
                Err(e) => Err(Error::ResolveError(e)),
            })
            .boxed(),
        Protocol::Dns4(ref name) => resolver
            .ipv4_lookup(name.clone().into_owned())
            .map(move |res| match res {
                Ok(ips) => {
                    let mut ips = ips.into_iter();
                    let one = ips
                        .next()
                        .expect("If there are no results, `Err(NoRecordsFound)` is expected.");
                    if let Some(two) = ips.next() {
                        Ok(Resolved::Many(
                            iter::once(one)
                                .chain(iter::once(two))
                                .chain(ips)
                                .map(Protocol::from)
                                .collect(),
                        ))
                    } else {
                        Ok(Resolved::One(Protocol::from(one)))
                    }
                }
                Err(e) => Err(Error::ResolveError(e)),
            })
            .boxed(),
        Protocol::Dns6(ref name) => resolver
            .ipv6_lookup(name.clone().into_owned())
            .map(move |res| match res {
                Ok(ips) => {
                    let mut ips = ips.into_iter();
                    let one = ips
                        .next()
                        .expect("If there are no results, `Err(NoRecordsFound)` is expected.");
                    if let Some(two) = ips.next() {
                        Ok(Resolved::Many(
                            iter::once(one)
                                .chain(iter::once(two))
                                .chain(ips)
                                .map(Protocol::from)
                                .collect(),
                        ))
                    } else {
                        Ok(Resolved::One(Protocol::from(one)))
                    }
                }
                Err(e) => Err(Error::ResolveError(e)),
            })
            .boxed(),
        Protocol::Dnsaddr(ref name) => {
            let name = [DNSADDR_PREFIX, name].concat();
            resolver
                .txt_lookup(name)
                .map(move |res| match res {
                    Ok(txts) => {
                        let mut addrs = Vec::new();
                        for txt in txts {
                            if let Some(chars) = txt.first() {
                                match parse_dnsaddr_txt(chars) {
                                    Err(e) => {
                                        tracing::debug!("Invalid TXT record: {:?}", e);
                                    }
                                    Ok(a) => {
                                        addrs.push(a);
                                    }
                                }
                            }
                        }
                        Ok(Resolved::Addrs(addrs))
                    }
                    Err(e) => Err(Error::ResolveError(e)),
                })
                .boxed()
        }
        proto => future::ready(Ok(Resolved::One(proto.clone()))).boxed(),
    }
}

/// Parses a `<character-string>` of a `dnsaddr` TXT record.
fn parse_dnsaddr_txt(txt: &[u8]) -> io::Result<Multiaddr> {
    let s = str::from_utf8(txt).map_err(invalid_data)?;
    match s.strip_prefix("dnsaddr=") {
        None => Err(invalid_data("Missing `dnsaddr=` prefix.")),
        Some(a) => Ok(Multiaddr::try_from(a).map_err(invalid_data)?),
    }
}

fn invalid_data(e: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, e)
}

#[async_trait::async_trait]
#[doc(hidden)]
pub trait Resolver {
    async fn lookup_ip(&self, name: String) -> Result<Vec<std::net::IpAddr>, ResolveError>;
    async fn ipv4_lookup(&self, name: String) -> Result<Vec<Ipv4Addr>, ResolveError>;
    async fn ipv6_lookup(&self, name: String) -> Result<Vec<Ipv6Addr>, ResolveError>;
    async fn txt_lookup(&self, name: String) -> Result<Vec<Vec<Vec<u8>>>, ResolveError>;
}

/// OS DNS resolver via `::tokio::net::lookup_host`.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemResolver;

#[async_trait]
impl Resolver for SystemResolver {
    async fn lookup_ip(&self, name: String) -> Result<Vec<std::net::IpAddr>, ResolveError> {
        let host = name.trim_end_matches('.').to_string();
        let mut out = Vec::new();
        for sa in ::tokio::net::lookup_host((host.as_str(), 0)).await? {
            out.push(sa.ip());
        }
        if out.is_empty() {
            return Err(ResolveErrorKind::Message("No matching records found.").into());
        }
        Ok(out)
    }

    async fn ipv4_lookup(&self, name: String) -> Result<Vec<Ipv4Addr>, ResolveError> {
        Ok(self
            .lookup_ip(name)
            .await?
            .into_iter()
            .filter_map(|ip| match ip {
                std::net::IpAddr::V4(v) => Some(v),
                std::net::IpAddr::V6(_) => None,
            })
            .collect())
    }

    async fn ipv6_lookup(&self, name: String) -> Result<Vec<Ipv6Addr>, ResolveError> {
        Ok(self
            .lookup_ip(name)
            .await?
            .into_iter()
            .filter_map(|ip| match ip {
                std::net::IpAddr::V6(v) => Some(v),
                std::net::IpAddr::V4(_) => None,
            })
            .collect())
    }

    async fn txt_lookup(&self, _name: String) -> Result<Vec<Vec<Vec<u8>>>, ResolveError> {
        // OS resolver has no TXT API; dnsaddr records are skipped.
        Ok(Vec::new())
    }
}
