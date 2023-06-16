//! Metrics-related functionality.

use prometheus::{Encoder, TextEncoder};
use serde::Serialize;
use std::any::TypeId;
use std::io;

pub(super) mod init;

#[doc(hidden)]
pub mod internal;

use internal::{
    collect_info_metrics, encode_registry, ErasedInfoMetric, INFO_REGISTRY, OPT_REGISTRY, REGISTRY,
};

/// Collects all metrics in a byte buffer.
pub fn collect(buffer: &mut Vec<u8>, collect_optional: bool) -> io::Result<()> {
    collect_info_metrics(buffer)?;

    encode_registry(buffer, &REGISTRY.read())?;

    if collect_optional {
        encode_registry(buffer, &OPT_REGISTRY.read())?;
    }

    TextEncoder::new()
        .encode(&prometheus::gather(), buffer)
        .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;

    Ok(())
}

/// A macro that allows to define Prometheus metrics.
///
/// The macro is a proc macro attribute that should be put on a module containing
/// bodyless functions. Each bodyless function corresponds to a single metric, whose
/// name becomes `<global prefix>_<module name>_<bodyless function name>`.
///
/// Arguments of the bodyless functions become labels for that metric.
///
/// The metric types must implement [`prometheus_client::metrics::MetricType`], they
/// are reexported from this module for convenience:
///
/// * [`Counter`]
/// * [`Gauge`]
/// * [`Histogram`]
/// * [`TimeHistogram`]
///
/// The metrics associated with the functions are automatically registered in a global
/// registry, and they can be collected with the [`collect`] function.
///
/// # Example
///
/// ```
/// # // As rustdoc puts doc tests in `fn main()`, the implicit `use super::*;` inserted
/// # // in the metric mod doesn't see `SomeLabel`, so we wrap the entire test in a module.
/// # mod rustdoc_workaround {
/// use bedrock::telemetry::metrics::{metrics, Counter, Gauge, HistogramBuilder, TimeHistogram};
/// use serde_with::DisplayFromStr;
/// use std::net::IpAddr;
/// use std::io;
/// use std::sync::Arc;
///
/// mod labels {
///     use serde::Serialize;
///
///     #[derive(Clone, Eq, Hash, PartialEq, Serialize)]
///     #[serde(rename_all = "lowercase")]
///     pub enum IpVersion {
///         V4,
///         V6,
///     }
///
///     #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize)]
///     #[serde(rename_all = "lowercase")]
///     pub enum L4Protocol {
///         Tcp,
///         Udp,
///         Quic,
///         Unknown,
///     }
///
///     #[derive(Clone, Eq, Hash, PartialEq, Serialize)]
///     #[serde(rename_all = "lowercase")]
///     pub enum ProxiedProtocol {
///         Ip,
///         Tcp,
///         Udp,
///         Quic,
///         Unknown,
///     }
///
///     impl From<L4Protocol> for ProxiedProtocol {
///         fn from(l4: L4Protocol) -> Self {
///             match l4 {
///                 L4Protocol::Tcp => Self::Tcp,
///                 L4Protocol::Udp => Self::Udp,
///                 L4Protocol::Quic => Self::Quic,
///                 L4Protocol::Unknown => Self::Unknown,
///             }
///         }
///     }
/// }
///
/// // The generated module contains an implicit `use super::*;` statement.
/// #[metrics]
/// pub mod my_app_metrics {
///     /// Number of active client connections
///     pub fn client_connections_active(
///         // Labels with an anonymous reference type will get cloned.
///         endpoint: &Arc<String>,
///         protocol: labels::L4Protocol,
///         ip_version: labels::IpVersion,
///         ingress_ip: IpAddr,
///     ) -> Gauge;
///
///     /// Histogram of task schedule delays
///     // Use the `ctor` attribute to specify how the metric should be built.
///     // The value should implement `MetricConstructor<MetricType>`.
///     #[ctor = HistogramBuilder {
///         // 0 us to 10 ms
///         buckets: &[0.0, 1E-4, 2E-4, 3E-4, 4E-4, 5E-4, 6E-4, 7E-4, 8E-4, 9E-4, 1E-3, 1E-2, 2E-2, 4E-2, 8E-2, 1E-1, 1.0],
///     }]
///     pub fn tokio_runtime_task_schedule_delay_histogram(
///         task: &Arc<str>,
///     ) -> TimeHistogram;
///
///     /// Number of client connections
///     pub fn client_connections_total(
///         endpoint: &Arc<String>,
///         // Labels with type `impl Into<T>` will invoke `std::convert::Into<T>`.
///         protocol: impl Into<labels::ProxiedProtocol>,
///         ingress_ip: IpAddr,
///     ) -> Counter;
///
///     /// Tunnel transmit error count
///     pub fn tunnel_transmit_errors_total(
///         endpoint: &Arc<String>,
///         protocol: labels::L4Protocol,
///         ingress_ip: IpAddr,
///         // `serde_as` attribute is allowed without decorating the metric with `serde_with::serde_as`.
///         #[serde_as(as = "DisplayFromStr")]
///         kind: io::ErrorKind,
///         raw_os_error: i32,
///     ) -> Counter;
///
///     /// Number of stalled futures
///     pub fn debug_stalled_future_count(
///         // Labels with a `'static` lifetime are used as is, without cloning.
///         name: &'static str,
///     ) -> Counter;
///
///     /// Number of Proxy-Status serialization errors
///     // Metrics with no labels are also obviously supported.
///     pub fn proxy_status_serialization_error_count() -> Counter;
/// }
///
/// fn usage() {
///     let endpoint = Arc::new("http-over-tcp".to_owned());
///     let l4_protocol = labels::L4Protocol::Tcp;
///     let ingress_ip = "127.0.0.1".parse::<IpAddr>().unwrap();
///     
///     my_app_metrics::client_connections_total(
///         &endpoint,
///         l4_protocol,
///         ingress_ip,
///     ).inc();
///     
///     let client_connections_active = my_app_metrics::client_connections_active(
///         &endpoint,
///         l4_protocol,
///         labels::IpVersion::V4,
///         ingress_ip,
///     );
///     
///     client_connections_active.inc();
///     
///     my_app_metrics::proxy_status_serialization_error_count().inc();
///
///     client_connections_active.dec();
/// }
/// # }
/// ```
///
/// # Renamed or reexported crate
///
/// The macro will fail to compile if `bedrock` crate is reexported. However, the crate path
/// can be explicitly specified for the macro to workaround that:
///
/// ```
/// # mod rustdoc_workaround {
/// mod reexport {
///     pub use bedrock::*;
/// }
///
/// use self::reexport::telemetry::metrics::Counter;
///
/// #[reexport::telemetry::metrics::metrics(crate_path = "reexport")]
/// mod oxy {
///     /// Total number of tasks workers stole from each other.
///     fn tokio_runtime_total_task_steal_count() -> Counter;
/// }
/// # }
/// ```
pub use bedrock_macros::metrics;

/// A macro that allows to define a Prometheus info metric.
///
/// The metrics defined by this function should be used with
/// [`report_info`] and they can be collected with
/// the telemetry server.
///
/// The struct name becomes the metric name in `snake_case`,
/// and each field of the struct becomes a label.
///
/// # Simple example
///
/// See [`report_info`] for a simple example.
///
/// # Renaming the metric.
///
/// ```
/// use bedrock::telemetry::metrics::{info_metric, report_info};
///
/// /// Build information
/// #[info_metric(name = "build_info")]
/// struct BuildInformation {
///     version: &'static str,
/// }
///
/// report_info(BuildInformation {
///     version: "1.2.3",
/// });
/// ```
/// # Renamed or reexported crate
///
/// The macro will fail to compile if `bedrock` crate is reexported. However, the crate path
/// can be explicitly specified for the macro to workaround that:
///
/// ```
/// # mod rustdoc_workaround {
/// mod reexport {
///     pub use bedrock::*;
/// }
///
/// /// Build information
/// #[reexport::telemetry::metrics::info_metric(crate_path = "reexport")]
/// struct BuildInfo {
///     version: &'static str,
/// }
/// # }
/// ```
pub use bedrock_macros::info_metric;

pub use prometheus_client::metrics::family::MetricConstructor;
pub use prometheus_client::metrics::gauge::Gauge;
pub use prometheus_client::metrics::histogram::Histogram;
pub use prometools::histogram::{HistogramTimer, TimeHistogram};
pub use prometools::nonstandard::NonstandardUnsuffixedCounter as Counter;
pub use prometools::serde::Family;

/// Describes an info metric.
///
/// Info metrics are used to expose textual information, through the label set, which should not
/// change often during process lifetime. Common examples are an application's version, revision
/// control commit, and the version of a compiler.
pub trait InfoMetric: Serialize + Send + Sync + 'static {
    /// The name of the info metric.
    const NAME: &'static str;

    /// The help message of the info metric.
    const HELP: &'static str;
}

/// Registers an info metric, i.e. a gauge metric whose value is always 1, set at init time.
///
/// # Examples
///
/// ```
/// use bedrock::telemetry::metrics::{info_metric, report_info};
///
/// /// Build information
/// #[info_metric]
/// struct BuildInfo {
///     version: &'static str,
/// }
///
/// report_info(BuildInfo {
///     version: "1.2.3",
/// });
/// ```
pub fn report_info<M>(info_metric: impl Into<Box<M>>)
where
    M: InfoMetric,
{
    INFO_REGISTRY.write().insert(
        TypeId::of::<M>(),
        info_metric.into() as Box<dyn ErasedInfoMetric>,
    );
}

/// A builder suitable for [`Histogram`] and [`TimeHistogram`].
///
/// # Example
///
/// ```
/// # // As rustdoc puts doc tests in `fn main()`, the implicit `use super::*;` inserted
/// # // in the metric mod doesn't see `SomeLabel`, so we wrap the entire test in a module.
/// # mod rustdoc_workaround {
/// use bedrock::telemetry::metrics::{metrics, HistogramBuilder, TimeHistogram};
///
/// #[metrics]
/// pub mod my_app_metrics {
///     #[ctor = HistogramBuilder {
///         // 0 us to 10 ms
///         buckets: &[0.0, 1E-4, 2E-4, 3E-4, 4E-4, 5E-4, 6E-4, 7E-4, 8E-4, 9E-4, 1E-3, 1E-2, 2E-2, 4E-2, 8E-2, 1E-1, 1.0],
///     }]
///     pub fn tokio_runtime_task_schedule_delay_histogram(
///         task: String,
///     ) -> TimeHistogram;
/// }
/// # }
/// ```
#[derive(Clone)]
pub struct HistogramBuilder {
    /// The buckets of the histogram to be built.
    pub buckets: &'static [f64],
}

impl MetricConstructor<Histogram> for HistogramBuilder {
    fn new_metric(&self) -> Histogram {
        Histogram::new(self.buckets.iter().cloned())
    }
}

impl MetricConstructor<TimeHistogram> for HistogramBuilder {
    fn new_metric(&self) -> TimeHistogram {
        TimeHistogram::new(self.buckets.iter().cloned())
    }
}
