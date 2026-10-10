//! Python bindings for the [`saleae_rs`] crate: a `Session` talks to the headless Logic 2 automation
//! server (gRPC),
//! starting it in the background on first use (as the CLI does). Each call blocks on a runtime owned by the
//! `Session`, so no `asyncio` is needed on the Python side.
//!
//! Analyzers are added generically through [`Session::add_analyzer`] with a settings dict (the typed SPI/I2C/...
//! shorthands the CLI has are not wrapped yet, see `AGENTS.md` CLI-6); setting names are the ones Logic 2 shows
//! (`saleae analyzer list`, or the server's error message after a wrong name).

use pyo3::exceptions::PyException;
use pyo3::prelude::*;
use pyo3::types::PyDict;
use std::path::PathBuf;

pyo3::create_exception!(saleae_rs, SaleaeError, PyException);
pyo3::create_exception!(saleae_rs, NotFoundError, SaleaeError);
pyo3::create_exception!(saleae_rs, InvalidInputError, SaleaeError);
pyo3::create_exception!(saleae_rs, RpcError, SaleaeError);
pyo3::create_exception!(saleae_rs, ServerError, SaleaeError);
pyo3::create_exception!(saleae_rs, IoError, SaleaeError);

fn to_py(e: ::saleae_rs::Error) -> PyErr {
    match e {
        ::saleae_rs::Error::NotFound(m) => NotFoundError::new_err(m),
        ::saleae_rs::Error::InvalidInput(m) => InvalidInputError::new_err(m),
        ::saleae_rs::Error::Rpc { code, message } => {
            RpcError::new_err(format!("{message} ({code:?})"))
        }
        ::saleae_rs::Error::Server(m) => ServerError::new_err(m),
        ::saleae_rs::Error::Io(e) => IoError::new_err(e.to_string()),
        ::saleae_rs::Error::Json(e) => SaleaeError::new_err(e.to_string()),
        ::saleae_rs::Error::Transport(e) => ServerError::new_err(e.to_string()),
    }
}

/// A setting value from a Python dict: `int`, `bool`, `float` or `str` (the option text as shown in Logic 2).
fn setting_value(v: &Bound<'_, PyAny>) -> PyResult<::saleae_rs::pb::AnalyzerSettingValue> {
    use ::saleae_rs::pb::AnalyzerSettingValue;
    use ::saleae_rs::pb::analyzer_setting_value::Value;
    let value = if let Ok(b) = v.extract::<bool>() {
        Value::BoolValue(b)
    } else if let Ok(i) = v.extract::<i64>() {
        Value::Int64Value(i)
    } else if let Ok(f) = v.extract::<f64>() {
        Value::DoubleValue(f)
    } else if let Ok(s) = v.extract::<String>() {
        Value::StringValue(s)
    } else {
        return Err(InvalidInputError::new_err(
            "analyzer setting values must be int, bool, float or str",
        ));
    };
    Ok(AnalyzerSettingValue { value: Some(value) })
}

fn settings_from_dict(d: Option<&Bound<'_, PyDict>>) -> PyResult<::saleae_rs::analyzer::Overrides> {
    let mut out = ::saleae_rs::analyzer::Overrides::new();
    if let Some(d) = d {
        for (k, v) in d.iter() {
            out.insert(k.extract::<String>()?, setting_value(&v)?);
        }
    }
    Ok(out)
}

/// A connection to the headless automation server: starts it in the background on first use (unless
/// `no_launch`), same as the `saleae` CLI.
#[pyclass]
struct Session {
    rt: tokio::runtime::Runtime,
    inner: ::saleae_rs::server::Session,
}

#[pymethods]
impl Session {
    /// `addr`: `host:port` of the automation server (default `127.0.0.1:10430`). `sim_only`: start the server
    /// without USB scanning, so only simulated devices are available.
    #[new]
    #[pyo3(signature = (addr=None, no_launch=false, sim_only=false, server_bin=None))]
    fn new(
        addr: Option<String>,
        no_launch: bool,
        sim_only: bool,
        server_bin: Option<PathBuf>,
    ) -> PyResult<Self> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| ServerError::new_err(e.to_string()))?;
        let conn = ::saleae_rs::server::Conn {
            addr: addr.unwrap_or_else(|| ::saleae_rs::server::DEFAULT_ADDR.to_string()),
            no_launch,
            server_bin,
            no_usb: sim_only,
        };
        let inner = rt.block_on(conn.session(None)).map_err(to_py)?;
        Ok(Session { rt, inner })
    }

    /// The server's version string.
    #[getter]
    fn app_version(&self) -> &str {
        &self.inner.app_version
    }

    /// The server's process id (for `stop(session.server_pid)` after a test or a one-off script).
    #[getter]
    fn server_pid(&self) -> u64 {
        self.inner.server_pid
    }

    /// Real devices, and simulated ones unless `real_only`: `[{"id", "type", "simulated"}, ...]`.
    #[pyo3(signature = (real_only=false))]
    fn devices(&mut self, py: Python<'_>, real_only: bool) -> PyResult<Vec<Py<PyAny>>> {
        let devices = self
            .rt
            .block_on(::saleae_rs::device::list(&mut self.inner, real_only))
            .map_err(to_py)?;
        devices
            .into_iter()
            .map(|d| {
                let dict = PyDict::new(py);
                dict.set_item("id", d.id)?;
                dict.set_item("type", d.type_name)?;
                dict.set_item("simulated", d.simulated)?;
                Ok(dict.into_any().unbind())
            })
            .collect()
    }

    /// Records a timed capture; returns `{"capture", "device", "desc", "end"}`. Digital trigger mode is not
    /// exposed here yet; use the CLI (`saleae capture --trigger ...`) for that.
    #[pyo3(signature = (device=None, digital=None, analog=None, rate=10e6, analog_rate=1.5625e6,
                        threshold=None, duration=1.0, buffer_mb=0))]
    #[allow(clippy::too_many_arguments)]
    fn capture(
        &mut self,
        py: Python<'_>,
        device: Option<String>,
        digital: Option<Vec<u32>>,
        analog: Option<Vec<u32>>,
        rate: f64,
        analog_rate: f64,
        threshold: Option<f64>,
        duration: f64,
        buffer_mb: u32,
    ) -> PyResult<Py<PyAny>> {
        let opts = ::saleae_rs::capture::CaptureOptions {
            device,
            digital: digital.unwrap_or_default(),
            analog: analog.unwrap_or_default(),
            rate,
            analog_rate,
            threshold,
            duration,
            trigger: None,
            trim: None,
            glitch: vec![],
            buffer_mb,
        };
        let (rec, end) = self
            .rt
            .block_on(::saleae_rs::capture::run(&mut self.inner, &opts, &[]))
            .map_err(to_py)?;
        let dict = PyDict::new(py);
        dict.set_item("capture", rec.id)?;
        dict.set_item("device", rec.device)?;
        dict.set_item("desc", rec.desc)?;
        dict.set_item(
            "end",
            matches!(end, ::saleae_rs::capture::End::TriggerTimeout)
                .then_some("trigger_timeout")
                .unwrap_or("completed"),
        )?;
        Ok(dict.into_any().unbind())
    }

    /// Adds an analyzer by its Logic 2 name (`"I2C"`, `"SPI"`, ...; see `saleae analyzer list`), with its
    /// settings as a dict (`{"SDA": 0, "SCL": 1}`). Returns the analyzer id.
    #[pyo3(signature = (capture, name, settings=None, channels=None, label=None))]
    fn add_analyzer(
        &mut self,
        capture: u64,
        name: String,
        settings: Option<&Bound<'_, PyDict>>,
        channels: Option<Vec<u32>>,
        label: Option<String>,
    ) -> PyResult<u64> {
        let spec = ::saleae_rs::analyzer::Protocol::Other(::saleae_rs::analyzer::OtherOptions {
            name,
            channels: channels.unwrap_or_default(),
            overrides: settings_from_dict(settings)?,
        })
        .spec()
        .map_err(to_py)?;
        let rec = self
            .rt
            .block_on(::saleae_rs::analyzer::add(
                &mut self.inner,
                capture,
                &spec,
                label,
            ))
            .map_err(to_py)?;
        Ok(rec.id)
    }

    /// Removes an analyzer from a capture.
    fn remove_analyzer(&mut self, capture: u64, analyzer: u64) -> PyResult<()> {
        self.rt
            .block_on(::saleae_rs::analyzer::remove(
                &mut self.inner,
                capture,
                analyzer,
            ))
            .map_err(to_py)
    }

    /// Summarizes an analyzer's data table (as the CLI's `decode`/`summarize` print it).
    /// `kind`: `"i2c"`, `"spi"`, `"serial"`, `"can"` or `"other"` (default); for `"spi"`, `mosi`/`miso` say which
    /// data lines were wired. Returns `{"text": ..., "json": {...}}`.
    #[pyo3(signature = (capture, analyzer, kind="other", limit=40, csv=None, mosi=true, miso=true))]
    #[allow(clippy::too_many_arguments)]
    fn summarize(
        &mut self,
        py: Python<'_>,
        capture: u64,
        analyzer: u64,
        kind: &str,
        limit: usize,
        csv: Option<PathBuf>,
        mosi: bool,
        miso: bool,
    ) -> PyResult<Py<PyAny>> {
        let kind = match kind {
            "i2c" => ::saleae_rs::analyzer::Kind::I2c,
            "spi" => ::saleae_rs::analyzer::Kind::Spi { mosi, miso },
            "serial" => ::saleae_rs::analyzer::Kind::Serial,
            "can" => ::saleae_rs::analyzer::Kind::Can,
            "other" => ::saleae_rs::analyzer::Kind::Other,
            other => {
                return Err(InvalidInputError::new_err(format!(
                    "kind `{other}` is not i2c, spi, serial, can or other"
                )));
            }
        };
        let summary = self
            .rt
            .block_on(::saleae_rs::decode::summarize_analyzer(
                &mut self.inner,
                capture,
                analyzer,
                kind,
                limit,
                csv.as_deref(),
            ))
            .map_err(to_py)?;
        let dict = PyDict::new(py);
        dict.set_item("text", summary.text())?;
        dict.set_item("json", pythonize(py, &summary.json())?)?;
        Ok(dict.into_any().unbind())
    }

    /// Saves a capture as a `.sal` file (opens in Logic 2).
    fn save(&mut self, capture: u64, file: PathBuf) -> PyResult<()> {
        self.rt
            .block_on(::saleae_rs::capture::save(&mut self.inner, capture, &file))
            .map_err(to_py)
    }

    /// Loads a `.sal` capture file into the server; returns its capture id.
    fn load(&mut self, file: PathBuf) -> PyResult<u64> {
        self.rt
            .block_on(::saleae_rs::capture::load(&mut self.inner, &file))
            .map_err(to_py)
    }

    /// Stops (if still running) and closes a capture, freeing its memory in the server.
    fn close(&mut self, capture: u64) -> PyResult<()> {
        self.rt
            .block_on(::saleae_rs::capture::close(&mut self.inner, capture))
            .map_err(to_py)
    }
}

/// `serde_json::Value` as plain Python objects (dict / list / str / int / float / bool / None); good enough for
/// the JSON this crate produces (no bytes, no non-string map keys).
fn pythonize(py: Python<'_>, v: &serde_json::Value) -> PyResult<Py<PyAny>> {
    Ok(match v {
        serde_json::Value::Null => py.None(),
        serde_json::Value::Bool(b) => pyo3::types::PyBool::new(py, *b)
            .to_owned()
            .into_any()
            .unbind(),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i.into_pyobject(py)?.into_any().unbind()
            } else {
                n.as_f64()
                    .unwrap_or_default()
                    .into_pyobject(py)?
                    .into_any()
                    .unbind()
            }
        }
        serde_json::Value::String(s) => s.into_pyobject(py)?.into_any().unbind(),
        serde_json::Value::Array(a) => {
            let items: PyResult<Vec<Py<PyAny>>> = a.iter().map(|x| pythonize(py, x)).collect();
            items?.into_pyobject(py)?.into_any().unbind()
        }
        serde_json::Value::Object(o) => {
            let dict = PyDict::new(py);
            for (k, val) in o {
                dict.set_item(k, pythonize(py, val)?)?;
            }
            dict.into_any().unbind()
        }
    })
}

/// Installs the headless automation server for this platform into the CLI's data dir; returns the server binary
/// path. `build`: release build id from the Saleae forum post (default: the pinned one); `url`: a full download
/// URL instead; `zip`: an already downloaded zip file.
#[pyfunction]
#[pyo3(signature = (build=None, url=None, zip=None))]
fn install(build: Option<String>, url: Option<String>, zip: Option<PathBuf>) -> PyResult<PathBuf> {
    let url = match url {
        Some(u) => u,
        None => ::saleae_rs::server::download_url(
            build
                .as_deref()
                .unwrap_or(::saleae_rs::server::DEFAULT_BUILD),
        )
        .map_err(to_py)?,
    };
    ::saleae_rs::server::install(&url, zip.as_deref(), None).map_err(to_py)
}

/// Ends the server with the given pid, after checking that it is the automation server.
#[pyfunction]
fn stop(pid: u64) -> PyResult<()> {
    ::saleae_rs::server::kill(pid).map_err(to_py)
}

#[pymodule]
fn saleae_rs(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Session>()?;
    m.add_function(wrap_pyfunction!(install, m)?)?;
    m.add_function(wrap_pyfunction!(stop, m)?)?;
    m.add("SaleaeError", m.py().get_type::<SaleaeError>())?;
    m.add("NotFoundError", m.py().get_type::<NotFoundError>())?;
    m.add("InvalidInputError", m.py().get_type::<InvalidInputError>())?;
    m.add("RpcError", m.py().get_type::<RpcError>())?;
    m.add("ServerError", m.py().get_type::<ServerError>())?;
    m.add("IoError", m.py().get_type::<IoError>())?;
    m.add("DEFAULT_ADDR", ::saleae_rs::server::DEFAULT_ADDR)?;
    Ok(())
}
