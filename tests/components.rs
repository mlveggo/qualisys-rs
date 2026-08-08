//! Tests for the binary component decoders.

use qualisys::components::{Component, ComponentType, ImageFormat, Timecode};
use qualisys::{ByteOrder, DataFrame, Error, Packet, PacketType};

/// Little-endian payload builder.
#[derive(Default)]
struct Buf(Vec<u8>);

impl Buf {
    fn u8(mut self, v: u8) -> Self {
        self.0.push(v);
        self
    }
    fn u16(mut self, v: u16) -> Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn u32(mut self, v: u32) -> Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn u64(mut self, v: u64) -> Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn f32(mut self, v: f32) -> Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn raw(mut self, v: &[u8]) -> Self {
        self.0.extend_from_slice(v);
        self
    }
    fn done(self) -> Vec<u8> {
        self.0
    }
}

/// Wraps a component payload in its size and type header, then in a data frame,
/// then in a packet, and decodes the whole thing.
fn decode_component(kind: u32, payload: &[u8]) -> Result<Component, Error> {
    let mut frame = Buf::default()
        .u64(1234) // timestamp
        .u32(42) // frame number
        .u32(1) // component count
        .u32((payload.len() + 8) as u32)
        .u32(kind)
        .done();
    frame.extend_from_slice(payload);

    let mut packet = Buf::default()
        .u32((frame.len() + 8) as u32)
        .u32(PacketType::Data as u32)
        .done();
    packet.extend_from_slice(&frame);

    match Packet::decode(&packet, ByteOrder::Little)? {
        Packet::Data(frame) => Ok(frame.components.into_iter().next().expect("one component")),
        other => panic!("expected a data packet, got {other:?}"),
    }
}

#[test]
fn image_decodes_dimensions_and_payload() {
    // The width field sits at a fixed offset that is easy to get wrong; reading
    // it from a zero-length slice would panic on every image frame.
    let pixels = [0xDEu8, 0xAD, 0xBE, 0xEF, 0x01];
    let payload = Buf::default()
        .u32(1) // image count
        .u32(7) // camera id
        .u32(2) // format: JPG
        .u32(1920)
        .u32(1080)
        .f32(0.1)
        .f32(0.2)
        .f32(0.3)
        .f32(0.4)
        .u32(pixels.len() as u32)
        .raw(&pixels)
        .done();

    match decode_component(ComponentType::Image as u32, &payload).unwrap() {
        Component::Image(images) => {
            assert_eq!(images.images.len(), 1);
            let img = &images.images[0];
            assert_eq!(img.camera_id, 7);
            assert_eq!(img.format, ImageFormat::Jpg);
            assert_eq!(img.width, 1920);
            assert_eq!(img.height, 1080);
            assert_eq!(img.data, pixels.to_vec());
        }
        other => panic!("got {other:?}"),
    }
}

#[test]
fn image_rejects_a_payload_longer_than_the_component() {
    let payload = Buf::default()
        .u32(1)
        .u32(1)
        .u32(0)
        .u32(4)
        .u32(4)
        .f32(0.0)
        .f32(0.0)
        .f32(0.0)
        .f32(0.0)
        .u32(1 << 30) // claims a gigabyte that is not there
        .done();

    assert!(decode_component(ComponentType::Image as u32, &payload).is_err());
}

#[test]
fn timecode_advances_between_entries() {
    // Every entry is a fixed 12 bytes. Reading them all from the same offsets
    // would report the first timecode repeated N times.
    fn smpte(h: u32, m: u32, s: u32, f: u32, sub: u32) -> u32 {
        h | m << 5 | s << 11 | f << 17 | sub << 22
    }

    let payload = Buf::default()
        .u32(2)
        .u32(0)
        .u32(0)
        .u32(smpte(1, 2, 3, 4, 5))
        .u32(0)
        .u32(0)
        .u32(smpte(6, 7, 8, 9, 10))
        .done();

    match decode_component(ComponentType::Timecode as u32, &payload).unwrap() {
        Component::Timecode(tc) => {
            assert_eq!(tc.timecodes.len(), 2);
            let (Timecode::Smpte(first), Timecode::Smpte(second)) =
                (tc.timecodes[0], tc.timecodes[1])
            else {
                panic!("expected two SMPTE timecodes, got {:?}", tc.timecodes);
            };
            assert_eq!(
                (first.hours, first.minutes, first.frames, first.sub_frame),
                (1, 2, 4, 5)
            );
            assert_eq!(
                (
                    second.hours,
                    second.minutes,
                    second.frames,
                    second.sub_frame
                ),
                (6, 7, 9, 10),
                "entries were not advanced"
            );
        }
        other => panic!("got {other:?}"),
    }
}

#[test]
fn smpte_normalized_sub_frame() {
    use qualisys::components::SmpteTime;
    let tc = SmpteTime {
        sub_frame: 3,
        ..Default::default()
    };
    assert_eq!(tc.normalized_sub_frame(120, 30), 0.75);
    // Degenerate frequencies must not divide by zero or return nonsense.
    assert_eq!(tc.normalized_sub_frame(0, 30), 0.0);
    assert_eq!(tc.normalized_sub_frame(30, 0), 0.0);
    assert_eq!(tc.normalized_sub_frame(30, 120), 0.0);
}

#[test]
fn markers_3d_variants_use_the_right_stride() {
    let cases = [
        (ComponentType::Markers3d, false, false),
        (ComponentType::Markers3dResidual, false, true),
        (ComponentType::Markers3dNoLabels, true, false),
        (ComponentType::Markers3dNoLabelsResidual, true, true),
    ];

    for (kind, with_id, with_residual) in cases {
        let mut b = Buf::default().u32(2).u16(11).u16(22);
        for i in 0..2u32 {
            b = b.f32(i as f32).f32(i as f32 + 0.5).f32(i as f32 + 0.25);
            if with_id {
                b = b.u32(100 + i);
            }
            if with_residual {
                b = b.f32(i as f32 * 2.0);
            }
        }

        let markers = match decode_component(kind as u32, &b.done()).unwrap() {
            Component::Markers3d(m)
            | Component::Markers3dResidual(m)
            | Component::Markers3dNoLabels(m)
            | Component::Markers3dNoLabelsResidual(m) => m,
            other => panic!("{kind:?} decoded as {other:?}"),
        };

        assert_eq!(markers.drop_rate, 11, "{kind:?}");
        assert_eq!(markers.out_of_sync_rate, 22, "{kind:?}");
        assert_eq!(markers.markers.len(), 2, "{kind:?}");
        assert_eq!(markers.markers[1].position.x, 1.0, "{kind:?}");
        assert_eq!(
            markers.markers[1].id,
            with_id.then_some(101),
            "{kind:?} id handling"
        );
        assert_eq!(
            markers.markers[1].residual,
            with_residual.then_some(2.0),
            "{kind:?} residual handling"
        );
    }
}

#[test]
fn markers_2d_camera_stride() {
    // Two cameras with different marker counts exercises the 5 byte camera
    // header plus 12 bytes per marker.
    let payload = Buf::default()
        .u32(2)
        .u16(0)
        .u16(0)
        .u32(2)
        .u8(0x01)
        .u32(10)
        .u32(20)
        .u16(3)
        .u16(4)
        .u32(11)
        .u32(21)
        .u16(5)
        .u16(6)
        .u32(1)
        .u8(0x02)
        .u32(30)
        .u32(40)
        .u16(7)
        .u16(8)
        .done();

    match decode_component(ComponentType::Markers2d as u32, &payload).unwrap() {
        Component::Markers2d(m) => {
            assert_eq!(m.cameras.len(), 2);
            assert_eq!(m.cameras[0].markers.len(), 2);
            assert_eq!(m.cameras[1].markers.len(), 1);
            assert_eq!(m.cameras[1].status, 0x02);
            let marker = m.cameras[1].markers[0];
            assert_eq!(
                (marker.x, marker.y, marker.diameter_x, marker.diameter_y),
                (30, 40, 7, 8)
            );
        }
        other => panic!("got {other:?}"),
    }
}

#[test]
fn analog_groups_samples_by_channel() {
    // The wire format stores all samples of channel 0, then all of channel 1,
    // rather than interleaving them.
    let payload = Buf::default()
        .u32(1)
        .u32(5) // device id
        .u32(2) // channels
        .u32(3) // samples per channel
        .u32(42) // sample number
        .f32(1.0)
        .f32(2.0)
        .f32(3.0)
        .f32(4.0)
        .f32(5.0)
        .f32(6.0)
        .done();

    match decode_component(ComponentType::Analog as u32, &payload).unwrap() {
        Component::Analog(a) => {
            let device = &a.devices[0];
            assert_eq!(device.id, 5);
            assert_eq!(device.sample_number, 42);
            assert_eq!(device.channels.len(), 2);
            assert_eq!(device.channels[0], vec![1.0, 2.0, 3.0]);
            assert_eq!(device.channels[1], vec![4.0, 5.0, 6.0]);
        }
        other => panic!("got {other:?}"),
    }
}

#[test]
fn force_plate_samples() {
    let mut b = Buf::default().u32(1).u32(3).u32(2).u32(77);
    for i in 0..2 {
        let base = (i * 10) as f32;
        for offset in 0..9 {
            b = b.f32(base + offset as f32);
        }
    }

    match decode_component(ComponentType::Force as u32, &b.done()).unwrap() {
        Component::Force(f) => {
            let plate = &f.plates[0];
            assert_eq!(plate.id, 3);
            assert_eq!(plate.force_number, 77);
            assert_eq!(plate.samples.len(), 2);
            assert_eq!(plate.samples[1].center_of_pressure.z, 18.0);
        }
        other => panic!("got {other:?}"),
    }
}

#[test]
fn skeleton_segments() {
    let mut b = Buf::default().u32(2);
    b = b.u32(2);
    b = b
        .u32(1)
        .f32(1.0)
        .f32(2.0)
        .f32(3.0)
        .f32(0.0)
        .f32(0.0)
        .f32(0.0)
        .f32(1.0);
    b = b
        .u32(2)
        .f32(4.0)
        .f32(5.0)
        .f32(6.0)
        .f32(0.0)
        .f32(0.0)
        .f32(1.0)
        .f32(0.0);
    b = b.u32(1);
    b = b
        .u32(9)
        .f32(7.0)
        .f32(8.0)
        .f32(9.0)
        .f32(1.0)
        .f32(0.0)
        .f32(0.0)
        .f32(0.0);

    match decode_component(ComponentType::Skeleton as u32, &b.done()).unwrap() {
        Component::Skeleton(s) => {
            assert_eq!(s.skeletons.len(), 2);
            assert_eq!(s.skeletons[0].segments.len(), 2);
            assert_eq!(s.skeletons[1].segments.len(), 1);
            let segment = s.skeletons[1].segments[0];
            assert_eq!(segment.id, 9);
            assert_eq!(segment.position.x, 7.0);
            assert_eq!(segment.rotation, [1.0, 0.0, 0.0, 0.0]);
        }
        other => panic!("got {other:?}"),
    }
}

#[test]
fn gaze_vector_device_with_no_samples_omits_the_sample_number() {
    // A device reporting zero samples writes only its 4 byte count, with no
    // sample number field. Getting that stride wrong desynchronises every
    // device that follows.
    let payload = Buf::default()
        .u32(2)
        .u32(0) // device 0: no samples
        .u32(1) // device 1: one sample
        .u32(99) // sample number
        .f32(1.0)
        .f32(2.0)
        .f32(3.0)
        .f32(4.0)
        .f32(5.0)
        .f32(6.0)
        .done();

    match decode_component(ComponentType::GazeVector as u32, &payload).unwrap() {
        Component::GazeVector(g) => {
            assert_eq!(g.devices.len(), 2);
            assert!(g.devices[0].samples.is_empty());
            assert_eq!(g.devices[1].sample_number, 99);
            assert_eq!(g.devices[1].samples[0].position.z, 6.0);
        }
        other => panic!("got {other:?}"),
    }
}

#[test]
fn eye_tracker_device_with_no_samples_omits_the_sample_number() {
    let payload = Buf::default()
        .u32(2)
        .u32(0)
        .u32(1)
        .u32(7)
        .f32(2.5)
        .f32(3.5)
        .done();

    match decode_component(ComponentType::EyeTracker as u32, &payload).unwrap() {
        Component::EyeTracker(e) => {
            assert_eq!(e.devices.len(), 2);
            assert!(e.devices[0].samples.is_empty());
            assert_eq!(e.devices[1].sample_number, 7);
            assert_eq!(e.devices[1].samples[0].right_pupil_diameter, 3.5);
        }
        other => panic!("got {other:?}"),
    }
}

#[test]
fn unknown_component_types_are_preserved_not_fatal() {
    // A newer QTM may stream a component this build has never seen. Failing the
    // whole frame would throw away the components it does understand.
    let mut frame = Buf::default()
        .u64(1)
        .u32(1)
        .u32(2) // two components
        // A valid 3D component with one marker.
        .u32(8 + 8 + 12)
        .u32(ComponentType::Markers3d as u32)
        .u32(1)
        .u16(0)
        .u16(0)
        .f32(1.0)
        .f32(2.0)
        .f32(3.0)
        // A component type from the future.
        .u32(8 + 6)
        .u32(9999)
        .raw(&[1, 2, 3, 4, 5, 6])
        .done();

    let mut packet = Buf::default()
        .u32((frame.len() + 8) as u32)
        .u32(PacketType::Data as u32)
        .done();
    packet.append(&mut frame);

    let Packet::Data(frame) = Packet::decode(&packet, ByteOrder::Little).unwrap() else {
        panic!("expected a data packet");
    };

    assert_eq!(frame.components.len(), 2);
    assert!(
        frame.markers_3d().is_some(),
        "known component still decoded"
    );
    assert_eq!(frame.unknown_component_types(), vec![9999]);
}

#[test]
fn zero_length_component_is_rejected() {
    // A zero size field would never advance the cursor, looping forever.
    let frame = Buf::default().u64(1).u32(1).u32(1).u32(0).u32(1).done();
    assert!(DataFrame::decode(&frame, ByteOrder::Little).is_err());
}

#[test]
fn component_claiming_more_bytes_than_remain_is_rejected() {
    let frame = Buf::default().u64(1).u32(1).u32(1).u32(4096).u32(1).done();
    assert!(DataFrame::decode(&frame, ByteOrder::Little).is_err());
}

#[test]
fn components_cannot_read_into_their_neighbour() {
    // The first component claims five markers but carries one. It must fail
    // rather than consuming the next component's bytes.
    let mut frame = Buf::default()
        .u64(1)
        .u32(1)
        .u32(2)
        .u32(8 + 8 + 12)
        .u32(ComponentType::Markers3d as u32)
        .u32(5) // claims five markers
        .u16(0)
        .u16(0)
        .f32(1.0)
        .f32(2.0)
        .f32(3.0)
        .done();
    frame.extend_from_slice(
        &Buf::default()
            .u32(8 + 200)
            .u32(ComponentType::Bodies6d as u32)
            .done(),
    );
    frame.extend_from_slice(&[0u8; 200]);

    assert!(DataFrame::decode(&frame, ByteOrder::Little).is_err());
}

/// Feeds every decoder progressively truncated payloads.
///
/// Nothing here may panic: a malformed frame from the network must surface as
/// an error, not take down the process.
#[test]
fn truncated_payloads_never_panic() {
    let kinds = [
        ComponentType::Markers3d,
        ComponentType::Markers3dResidual,
        ComponentType::Markers3dNoLabels,
        ComponentType::Markers3dNoLabelsResidual,
        ComponentType::Bodies6d,
        ComponentType::Bodies6dResidual,
        ComponentType::Bodies6dEuler,
        ComponentType::Bodies6dEulerResidual,
        ComponentType::Markers2d,
        ComponentType::Markers2dLinearized,
        ComponentType::Analog,
        ComponentType::AnalogSingle,
        ComponentType::Force,
        ComponentType::ForceSingle,
        ComponentType::Image,
        ComponentType::GazeVector,
        ComponentType::EyeTracker,
        ComponentType::Timecode,
        ComponentType::Skeleton,
    ];

    let valid = Buf::default()
        .u32(2)
        .u16(1)
        .u16(2)
        .f32(1.0)
        .f32(2.0)
        .f32(3.0)
        .u32(4)
        .f32(5.0)
        .f32(6.0)
        .f32(7.0)
        .f32(8.0)
        .u32(9)
        .f32(10.0)
        .done();

    for kind in kinds {
        for n in 0..=valid.len() {
            // Errors are expected and fine; a panic would fail the test.
            let _ = decode_component(kind as u32, &valid[..n]);
        }
    }
}
