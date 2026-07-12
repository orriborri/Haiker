//! Hardened GPX parser supporting GPX 1.0 and 1.1.
//!
//! Uses quick-xml with XXE prevention, depth limits, and point/segment limits.

use quick_xml::events::Event;
use quick_xml::Reader;

/// Maximum XML nesting depth allowed.
const MAX_DEPTH: usize = 20;
/// Maximum number of track points allowed across all tracks.
const MAX_POINTS: usize = 500_000;
/// Maximum number of track segments allowed.
const MAX_SEGMENTS: usize = 10_000;
/// Maximum metadata string length (after trimming).
const MAX_METADATA_LEN: usize = 1000;

/// Stable error codes for GPX parse failures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GpxParseErrorCode {
    InvalidXml,
    UnsupportedVersion,
    TooManyPoints,
    TooManySegments,
    XmlTooDeep,
    ExternalEntity,
    InvalidCoordinates,
}

impl std::fmt::Display for GpxParseErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::InvalidXml => "INVALID_XML",
            Self::UnsupportedVersion => "UNSUPPORTED_VERSION",
            Self::TooManyPoints => "TOO_MANY_POINTS",
            Self::TooManySegments => "TOO_MANY_SEGMENTS",
            Self::XmlTooDeep => "XML_TOO_DEEP",
            Self::ExternalEntity => "EXTERNAL_ENTITY",
            Self::InvalidCoordinates => "INVALID_COORDINATES",
        };
        write!(f, "{s}")
    }
}

/// Error from GPX parsing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpxParseError {
    pub code: GpxParseErrorCode,
    pub message: String,
}

impl std::fmt::Display for GpxParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for GpxParseError {}

impl GpxParseError {
    fn new(code: GpxParseErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

/// Detected GPX format version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpxVersion {
    Gpx10,
    Gpx11,
}

impl std::fmt::Display for GpxVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Gpx10 => write!(f, "1.0"),
            Self::Gpx11 => write!(f, "1.1"),
        }
    }
}

/// Metadata extracted from the GPX file.
#[derive(Debug, Clone, Default)]
pub struct GpxMetadata {
    pub name: Option<String>,
    pub description: Option<String>,
    pub time: Option<String>,
}

/// A single track point with coordinates and optional elevation/time.
#[derive(Debug, Clone)]
pub struct GpxTrackPoint {
    pub lat: f64,
    pub lon: f64,
    pub elevation: Option<f64>,
    pub time: Option<String>,
}

/// A track segment containing ordered track points.
#[derive(Debug, Clone)]
pub struct GpxTrackSegment {
    pub points: Vec<GpxTrackPoint>,
}

/// A track containing segments.
#[derive(Debug, Clone)]
pub struct GpxTrack {
    pub name: Option<String>,
    pub segments: Vec<GpxTrackSegment>,
}

/// The result of successfully parsing a GPX file.
#[derive(Debug, Clone)]
pub struct GpxParseResult {
    pub version: GpxVersion,
    pub metadata: GpxMetadata,
    pub tracks: Vec<GpxTrack>,
    pub total_points: usize,
}

/// Sanitize a metadata string: trim whitespace, strip control characters,
/// and truncate to MAX_METADATA_LEN.
fn sanitize_metadata(s: &str) -> String {
    let trimmed = s.trim();
    let cleaned: String = trimmed.chars().filter(|c| !c.is_control()).collect();
    if cleaned.len() > MAX_METADATA_LEN {
        cleaned.chars().take(MAX_METADATA_LEN).collect()
    } else {
        cleaned
    }
}

/// Parse a GPX document from raw XML bytes.
pub fn parse_gpx(input: &[u8]) -> Result<GpxParseResult, GpxParseError> {
    // Check for XXE patterns in raw input
    let input_str = std::str::from_utf8(input).map_err(|_| {
        GpxParseError::new(GpxParseErrorCode::InvalidXml, "input is not valid UTF-8")
    })?;

    if input_str.contains("<!DOCTYPE") || input_str.contains("<!ENTITY") {
        return Err(GpxParseError::new(
            GpxParseErrorCode::ExternalEntity,
            "DOCTYPE/ENTITY declarations are not allowed",
        ));
    }

    let mut reader = Reader::from_str(input_str);
    reader.config_mut().trim_text(true);

    let mut depth: usize = 0;
    let mut version: Option<GpxVersion> = None;
    let mut metadata = GpxMetadata::default();
    let mut tracks: Vec<GpxTrack> = Vec::new();
    let mut total_points: usize = 0;
    let mut total_segments: usize = 0;

    // Parsing state
    let mut in_metadata = false;
    let mut in_trk = false;
    let mut in_trkseg = false;
    let mut in_trkpt = false;
    let mut current_track: Option<GpxTrack> = None;
    let mut current_segment: Option<GpxTrackSegment> = None;
    let mut current_point: Option<GpxTrackPoint> = None;
    let mut current_text_target: Option<TextTarget> = None;
    let mut buf = Vec::new();

    #[derive(Debug)]
    enum TextTarget {
        MetadataName,
        MetadataDesc,
        MetadataTime,
        TrackName,
        PointEle,
        PointTime,
    }

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                depth += 1;
                if depth > MAX_DEPTH {
                    return Err(GpxParseError::new(
                        GpxParseErrorCode::XmlTooDeep,
                        format!("XML depth exceeded maximum of {MAX_DEPTH}"),
                    ));
                }

                let local_name = e.local_name();
                let tag = std::str::from_utf8(local_name.as_ref()).unwrap_or("");

                match tag {
                    "gpx" => {
                        // Extract version attribute
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"version" {
                                let val = std::str::from_utf8(&attr.value).unwrap_or("");
                                version = Some(match val {
                                    "1.0" => GpxVersion::Gpx10,
                                    "1.1" => GpxVersion::Gpx11,
                                    _ => {
                                        return Err(GpxParseError::new(
                                            GpxParseErrorCode::UnsupportedVersion,
                                            format!("unsupported GPX version: {val}"),
                                        ));
                                    }
                                });
                            }
                        }
                    }
                    "metadata" => {
                        in_metadata = true;
                    }
                    "name" => {
                        if in_trkpt {
                            // ignore name inside trkpt
                        } else if in_trk {
                            current_text_target = Some(TextTarget::TrackName);
                        } else if in_metadata || depth == 2 {
                            // GPX 1.0 has name directly under <gpx>
                            current_text_target = Some(TextTarget::MetadataName);
                        }
                    }
                    "desc" => {
                        if in_metadata || (!in_trk && depth == 2) {
                            current_text_target = Some(TextTarget::MetadataDesc);
                        }
                    }
                    "time" => {
                        if in_trkpt {
                            current_text_target = Some(TextTarget::PointTime);
                        } else if in_metadata || depth == 2 {
                            current_text_target = Some(TextTarget::MetadataTime);
                        }
                    }
                    "trk" => {
                        in_trk = true;
                        current_track = Some(GpxTrack {
                            name: None,
                            segments: Vec::new(),
                        });
                    }
                    "trkseg" => {
                        in_trkseg = true;
                        total_segments += 1;
                        if total_segments > MAX_SEGMENTS {
                            return Err(GpxParseError::new(
                                GpxParseErrorCode::TooManySegments,
                                format!("exceeded maximum of {MAX_SEGMENTS} segments"),
                            ));
                        }
                        current_segment = Some(GpxTrackSegment { points: Vec::new() });
                    }
                    "trkpt" => {
                        in_trkpt = true;
                        total_points += 1;
                        if total_points > MAX_POINTS {
                            return Err(GpxParseError::new(
                                GpxParseErrorCode::TooManyPoints,
                                format!("exceeded maximum of {MAX_POINTS} track points"),
                            ));
                        }

                        let mut lat: Option<f64> = None;
                        let mut lon: Option<f64> = None;
                        for attr in e.attributes().flatten() {
                            let key = std::str::from_utf8(attr.key.as_ref()).unwrap_or("");
                            let val = std::str::from_utf8(&attr.value).unwrap_or("");
                            match key {
                                "lat" => {
                                    lat = Some(val.parse::<f64>().map_err(|_| {
                                        GpxParseError::new(
                                            GpxParseErrorCode::InvalidCoordinates,
                                            format!("invalid latitude: {val}"),
                                        )
                                    })?);
                                }
                                "lon" => {
                                    lon = Some(val.parse::<f64>().map_err(|_| {
                                        GpxParseError::new(
                                            GpxParseErrorCode::InvalidCoordinates,
                                            format!("invalid longitude: {val}"),
                                        )
                                    })?);
                                }
                                _ => {}
                            }
                        }

                        let lat = lat.ok_or_else(|| {
                            GpxParseError::new(
                                GpxParseErrorCode::InvalidCoordinates,
                                "trkpt missing lat attribute",
                            )
                        })?;
                        let lon = lon.ok_or_else(|| {
                            GpxParseError::new(
                                GpxParseErrorCode::InvalidCoordinates,
                                "trkpt missing lon attribute",
                            )
                        })?;

                        if !(-90.0..=90.0).contains(&lat) {
                            return Err(GpxParseError::new(
                                GpxParseErrorCode::InvalidCoordinates,
                                format!("latitude {lat} out of range [-90, 90]"),
                            ));
                        }
                        if !(-180.0..=180.0).contains(&lon) {
                            return Err(GpxParseError::new(
                                GpxParseErrorCode::InvalidCoordinates,
                                format!("longitude {lon} out of range [-180, 180]"),
                            ));
                        }

                        current_point = Some(GpxTrackPoint {
                            lat,
                            lon,
                            elevation: None,
                            time: None,
                        });
                    }
                    "ele" => {
                        if in_trkpt {
                            current_text_target = Some(TextTarget::PointEle);
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::End(ref e)) => {
                let local_name = e.local_name();
                let tag = std::str::from_utf8(local_name.as_ref()).unwrap_or("");

                match tag {
                    "metadata" => {
                        in_metadata = false;
                    }
                    "trk" => {
                        in_trk = false;
                        if let Some(track) = current_track.take() {
                            tracks.push(track);
                        }
                    }
                    "trkseg" => {
                        in_trkseg = false;
                        if let Some(seg) = current_segment.take() {
                            if let Some(ref mut track) = current_track {
                                track.segments.push(seg);
                            }
                        }
                    }
                    "trkpt" => {
                        in_trkpt = false;
                        if let Some(pt) = current_point.take() {
                            if let Some(ref mut seg) = current_segment {
                                seg.points.push(pt);
                            }
                        }
                    }
                    _ => {
                        current_text_target = None;
                    }
                }

                depth = depth.saturating_sub(1);
            }
            Ok(Event::Empty(ref e)) => {
                // Self-closing tags like <trkpt ... />
                let local_name = e.local_name();
                let tag = std::str::from_utf8(local_name.as_ref()).unwrap_or("");

                if tag == "trkpt" && in_trkseg {
                    total_points += 1;
                    if total_points > MAX_POINTS {
                        return Err(GpxParseError::new(
                            GpxParseErrorCode::TooManyPoints,
                            format!("exceeded maximum of {MAX_POINTS} track points"),
                        ));
                    }

                    let mut lat: Option<f64> = None;
                    let mut lon: Option<f64> = None;
                    for attr in e.attributes().flatten() {
                        let key = std::str::from_utf8(attr.key.as_ref()).unwrap_or("");
                        let val = std::str::from_utf8(&attr.value).unwrap_or("");
                        match key {
                            "lat" => {
                                lat = Some(val.parse::<f64>().map_err(|_| {
                                    GpxParseError::new(
                                        GpxParseErrorCode::InvalidCoordinates,
                                        format!("invalid latitude: {val}"),
                                    )
                                })?);
                            }
                            "lon" => {
                                lon = Some(val.parse::<f64>().map_err(|_| {
                                    GpxParseError::new(
                                        GpxParseErrorCode::InvalidCoordinates,
                                        format!("invalid longitude: {val}"),
                                    )
                                })?);
                            }
                            _ => {}
                        }
                    }

                    let lat = lat.ok_or_else(|| {
                        GpxParseError::new(
                            GpxParseErrorCode::InvalidCoordinates,
                            "trkpt missing lat attribute",
                        )
                    })?;
                    let lon = lon.ok_or_else(|| {
                        GpxParseError::new(
                            GpxParseErrorCode::InvalidCoordinates,
                            "trkpt missing lon attribute",
                        )
                    })?;

                    if !(-90.0..=90.0).contains(&lat) {
                        return Err(GpxParseError::new(
                            GpxParseErrorCode::InvalidCoordinates,
                            format!("latitude {lat} out of range [-90, 90]"),
                        ));
                    }
                    if !(-180.0..=180.0).contains(&lon) {
                        return Err(GpxParseError::new(
                            GpxParseErrorCode::InvalidCoordinates,
                            format!("longitude {lon} out of range [-180, 180]"),
                        ));
                    }

                    if let Some(ref mut seg) = current_segment {
                        seg.points.push(GpxTrackPoint {
                            lat,
                            lon,
                            elevation: None,
                            time: None,
                        });
                    }
                }
            }
            Ok(Event::Text(ref e)) => {
                if let Some(ref target) = current_text_target {
                    let text = e.unescape().unwrap_or_default().to_string();
                    match target {
                        TextTarget::MetadataName => {
                            metadata.name = Some(sanitize_metadata(&text));
                        }
                        TextTarget::MetadataDesc => {
                            metadata.description = Some(sanitize_metadata(&text));
                        }
                        TextTarget::MetadataTime => {
                            metadata.time = Some(text.trim().to_string());
                        }
                        TextTarget::TrackName => {
                            if let Some(ref mut track) = current_track {
                                track.name = Some(sanitize_metadata(&text));
                            }
                        }
                        TextTarget::PointEle => {
                            if let Some(ref mut pt) = current_point {
                                pt.elevation = text.trim().parse::<f64>().ok();
                            }
                        }
                        TextTarget::PointTime => {
                            if let Some(ref mut pt) = current_point {
                                pt.time = Some(text.trim().to_string());
                            }
                        }
                    }
                    current_text_target = None;
                }
            }
            Ok(Event::Eof) => break,
            Ok(Event::DocType(_)) => {
                return Err(GpxParseError::new(
                    GpxParseErrorCode::ExternalEntity,
                    "DOCTYPE declarations are not allowed",
                ));
            }
            Err(_) => {
                return Err(GpxParseError::new(
                    GpxParseErrorCode::InvalidXml,
                    "failed to parse XML",
                ));
            }
            _ => {}
        }
        buf.clear();
    }

    let version = version.ok_or_else(|| {
        GpxParseError::new(
            GpxParseErrorCode::InvalidXml,
            "no <gpx> element with version attribute found",
        )
    })?;

    // Suppress unused variable warnings for state tracking
    let _ = in_trkseg;

    Ok(GpxParseResult {
        version,
        metadata,
        tracks,
        total_points,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_GPX_11: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<gpx version="1.1" creator="test"
     xmlns="http://www.topografix.com/GPX/1/1">
  <metadata>
    <name>Morning Hike</name>
    <desc>A nice trail run</desc>
    <time>2024-01-15T08:30:00Z</time>
  </metadata>
  <trk>
    <name>Track 1</name>
    <trkseg>
      <trkpt lat="47.1234" lon="11.5678">
        <ele>1200.5</ele>
        <time>2024-01-15T08:30:00Z</time>
      </trkpt>
      <trkpt lat="47.1235" lon="11.5679">
        <ele>1201.0</ele>
        <time>2024-01-15T08:30:05Z</time>
      </trkpt>
    </trkseg>
  </trk>
</gpx>"#;

    const VALID_GPX_10: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<gpx version="1.0" creator="test"
     xmlns="http://www.topografix.com/GPX/1/0">
  <name>Old Format Hike</name>
  <time>2023-06-01T10:00:00Z</time>
  <trk>
    <trkseg>
      <trkpt lat="46.5000" lon="10.2000">
        <ele>800.0</ele>
      </trkpt>
      <trkpt lat="46.5001" lon="10.2001">
        <ele>801.0</ele>
      </trkpt>
    </trkseg>
  </trk>
</gpx>"#;

    #[test]
    fn parse_valid_gpx_11() {
        let result = parse_gpx(VALID_GPX_11.as_bytes()).unwrap();
        assert_eq!(result.version, GpxVersion::Gpx11);
        assert_eq!(result.metadata.name.as_deref(), Some("Morning Hike"));
        assert_eq!(
            result.metadata.description.as_deref(),
            Some("A nice trail run")
        );
        assert_eq!(
            result.metadata.time.as_deref(),
            Some("2024-01-15T08:30:00Z")
        );
        assert_eq!(result.tracks.len(), 1);
        assert_eq!(result.tracks[0].name.as_deref(), Some("Track 1"));
        assert_eq!(result.tracks[0].segments.len(), 1);
        assert_eq!(result.tracks[0].segments[0].points.len(), 2);
        assert_eq!(result.total_points, 2);

        let pt = &result.tracks[0].segments[0].points[0];
        assert!((pt.lat - 47.1234).abs() < 1e-6);
        assert!((pt.lon - 11.5678).abs() < 1e-6);
        assert!((pt.elevation.unwrap() - 1200.5).abs() < 1e-6);
        assert_eq!(pt.time.as_deref(), Some("2024-01-15T08:30:00Z"));
    }

    #[test]
    fn parse_valid_gpx_10() {
        let result = parse_gpx(VALID_GPX_10.as_bytes()).unwrap();
        assert_eq!(result.version, GpxVersion::Gpx10);
        assert_eq!(result.metadata.name.as_deref(), Some("Old Format Hike"));
        assert_eq!(result.tracks.len(), 1);
        assert_eq!(result.tracks[0].segments[0].points.len(), 2);
    }

    #[test]
    fn reject_invalid_xml() {
        let bad = b"<gpx version=\"1.1\"><not closed";
        let err = parse_gpx(bad).unwrap_err();
        assert_eq!(err.code, GpxParseErrorCode::InvalidXml);
    }

    #[test]
    fn reject_unsupported_version() {
        let gpx = r#"<?xml version="1.0"?><gpx version="2.0"><trk><trkseg></trkseg></trk></gpx>"#;
        let err = parse_gpx(gpx.as_bytes()).unwrap_err();
        assert_eq!(err.code, GpxParseErrorCode::UnsupportedVersion);
    }

    #[test]
    fn reject_xxe_doctype() {
        let xxe = r#"<?xml version="1.0"?>
<!DOCTYPE foo [<!ENTITY xxe SYSTEM "file:///etc/passwd">]>
<gpx version="1.1"><trk><trkseg><trkpt lat="0" lon="0"/></trkseg></trk></gpx>"#;
        let err = parse_gpx(xxe.as_bytes()).unwrap_err();
        assert_eq!(err.code, GpxParseErrorCode::ExternalEntity);
    }

    #[test]
    fn reject_xxe_entity() {
        let xxe = r#"<?xml version="1.0"?>
<!ENTITY xxe "malicious">
<gpx version="1.1"><trk><trkseg><trkpt lat="0" lon="0"/></trkseg></trk></gpx>"#;
        let err = parse_gpx(xxe.as_bytes()).unwrap_err();
        assert_eq!(err.code, GpxParseErrorCode::ExternalEntity);
    }

    #[test]
    fn reject_invalid_coordinates_out_of_range() {
        let gpx = r#"<?xml version="1.0"?>
<gpx version="1.1"><trk><trkseg>
  <trkpt lat="91.0" lon="0.0"/>
</trkseg></trk></gpx>"#;
        let err = parse_gpx(gpx.as_bytes()).unwrap_err();
        assert_eq!(err.code, GpxParseErrorCode::InvalidCoordinates);
    }

    #[test]
    fn reject_invalid_longitude_out_of_range() {
        let gpx = r#"<?xml version="1.0"?>
<gpx version="1.1"><trk><trkseg>
  <trkpt lat="45.0" lon="181.0"/>
</trkseg></trk></gpx>"#;
        let err = parse_gpx(gpx.as_bytes()).unwrap_err();
        assert_eq!(err.code, GpxParseErrorCode::InvalidCoordinates);
    }

    #[test]
    fn reject_too_many_points() {
        // Generate a GPX with more than MAX_POINTS
        let mut gpx = String::from(r#"<?xml version="1.0"?><gpx version="1.1"><trk><trkseg>"#);
        for _ in 0..=MAX_POINTS {
            gpx.push_str(r#"<trkpt lat="47.0" lon="11.0"/>"#);
        }
        gpx.push_str("</trkseg></trk></gpx>");
        let err = parse_gpx(gpx.as_bytes()).unwrap_err();
        assert_eq!(err.code, GpxParseErrorCode::TooManyPoints);
    }

    #[test]
    fn reject_excessive_depth() {
        let mut gpx = String::from(r#"<?xml version="1.0"?><gpx version="1.1">"#);
        for _ in 0..25 {
            gpx.push_str("<a>");
        }
        for _ in 0..25 {
            gpx.push_str("</a>");
        }
        gpx.push_str("</gpx>");
        let err = parse_gpx(gpx.as_bytes()).unwrap_err();
        assert_eq!(err.code, GpxParseErrorCode::XmlTooDeep);
    }

    #[test]
    fn reject_too_many_segments() {
        let mut gpx = String::from(r#"<?xml version="1.0"?><gpx version="1.1"><trk>"#);
        for _ in 0..=MAX_SEGMENTS {
            gpx.push_str("<trkseg></trkseg>");
        }
        gpx.push_str("</trk></gpx>");
        let err = parse_gpx(gpx.as_bytes()).unwrap_err();
        assert_eq!(err.code, GpxParseErrorCode::TooManySegments);
    }

    #[test]
    fn sanitize_metadata_strips_control_chars() {
        let gpx = "<?xml version=\"1.0\"?>\n\
<gpx version=\"1.1\">\n\
  <metadata><name>Hello\x07World</name></metadata>\n\
  <trk><trkseg><trkpt lat=\"47.0\" lon=\"11.0\"/></trkseg></trk>\n\
</gpx>";
        let result = parse_gpx(gpx.as_bytes()).unwrap();
        assert_eq!(result.metadata.name.as_deref(), Some("HelloWorld"));
    }

    #[test]
    fn missing_lat_attribute_is_rejected() {
        let gpx = r#"<?xml version="1.0"?>
<gpx version="1.1"><trk><trkseg>
  <trkpt lon="11.0"/>
</trkseg></trk></gpx>"#;
        let err = parse_gpx(gpx.as_bytes()).unwrap_err();
        assert_eq!(err.code, GpxParseErrorCode::InvalidCoordinates);
    }

    #[test]
    fn self_closing_trkpt_elements() {
        let gpx = r#"<?xml version="1.0"?>
<gpx version="1.1"><trk><trkseg>
  <trkpt lat="47.0" lon="11.0"/>
  <trkpt lat="47.1" lon="11.1"/>
</trkseg></trk></gpx>"#;
        let result = parse_gpx(gpx.as_bytes()).unwrap();
        assert_eq!(result.total_points, 2);
        assert_eq!(result.tracks[0].segments[0].points.len(), 2);
    }
}
