use crate::{
    Result, RuntimeError, diagnostics::render_request, probe::ProbeRunner,
    progress::spin_with_ticks, simulate::SimulationRuntimeExt, source::inspect_source,
};
use quickbridge_core::{
    MediaInfo, PrepareEvent, PrepareStep, ProgressEvent, ProgressSink, SimulationScenario,
    SourceInspection,
};
use reqwest::Url;

#[derive(Debug)]
pub struct PrepareRequest {
    pub source_url: String,
    pub simulation: Option<SimulationScenario>,
    pub probe: ProbeRunner,
}

#[derive(Clone, Debug)]
pub struct PreparedSource {
    inspection: SourceInspection,
    media_info: MediaInfo,
}

impl PreparedSource {
    pub fn inspection(&self) -> &SourceInspection {
        &self.inspection
    }

    pub fn media_info(&self) -> &MediaInfo {
        &self.media_info
    }
}

pub async fn prepare_source<S>(request: PrepareRequest, sink: &mut S) -> Result<PreparedSource>
where
    S: ProgressSink<PrepareEvent, Error = RuntimeError>,
{
    sink.on_event(ProgressEvent::Started {
        step: PrepareStep::SourceUrl,
        details: vec![
            format!("Input: {}", request.source_url),
            String::from("Rule: a valid source URL is required"),
        ],
    })?;
    let parsed_url =
        Url::parse(&request.source_url).map_err(|_| RuntimeError::InvalidSourceUrl {
            source_url: request.source_url.clone(),
        })?;
    sink.on_event(ProgressEvent::Finished {
        step: PrepareStep::SourceUrl,
    })?;

    sink.on_event(ProgressEvent::Started {
        step: PrepareStep::TimeJumps,
        details: vec![
            render_request("HEAD", parsed_url.as_str(), None),
            render_request("GET", parsed_url.as_str(), Some("Range: bytes=0-0")),
        ],
    })?;
    let inspection = match &request.simulation {
        Some(simulation) => {
            spin_with_ticks(sink, simulation.inspect_source(parsed_url.as_str())).await?
        }
        None => {
            spin_with_ticks(sink, async {
                Ok::<_, RuntimeError>(inspect_source(&parsed_url).await)
            })
            .await?
        }
    };
    if inspection.seeking_enabled() {
        sink.on_event(ProgressEvent::Finished {
            step: PrepareStep::TimeJumps,
        })?;
    } else {
        sink.on_event(ProgressEvent::Warned {
            step: PrepareStep::TimeJumps,
            details: inspection
                .seek_warning()
                .map(|warning| vec![warning.to_string()])
                .unwrap_or_default(),
        })?;
    }
    sink.on_event(ProgressEvent::Finished {
        step: PrepareStep::SourceDetails,
    })?;

    sink.on_event(ProgressEvent::Started {
        step: PrepareStep::Tracks,
        details: render_probe_detail_lines(
            request.simulation.as_ref(),
            &request.probe,
            &request.source_url,
        ),
    })?;
    let media_info = match &request.simulation {
        Some(simulation) => {
            spin_with_ticks(sink, simulation.probe_source(&request.source_url)).await?
        }
        None => spin_with_ticks(sink, request.probe.probe(&request.source_url)).await?,
    };
    sink.on_event(ProgressEvent::Finished {
        step: PrepareStep::Tracks,
    })?;

    Ok(PreparedSource {
        inspection,
        media_info,
    })
}

fn render_probe_detail_lines(
    simulation: Option<&SimulationScenario>,
    probe: &ProbeRunner,
    source_url: &str,
) -> Vec<String> {
    match simulation {
        Some(simulation) => simulation
            .render_probe_commands(source_url)
            .into_iter()
            .map(|command| format!("Command: {command}"))
            .collect(),
        None => probe
            .render_probe_commands(source_url)
            .into_iter()
            .map(|command| format!("Command: {command}"))
            .collect(),
    }
}
