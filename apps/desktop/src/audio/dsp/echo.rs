//! Conservative playback-reference subtraction for speaker bleed into the mic.
//! This is not a mute-on-playback gate: independent microphone speech stays in
//! the residual. No voice identity is inferred from diarization or transcript text.
use std::collections::VecDeque;

const MAX_DELAY: usize = 5600; // 350 ms at 16 kHz, including device latency.
const STRIDE: usize = 8;

#[derive(Default)]
pub struct EchoSuppressor {
    reference: VecDeque<f32>,
}

impl EchoSuppressor {
    pub fn reset(&mut self) {
        self.reference.clear();
    }

    /// Input/output is interleaved mic-left/system-right at 16 kHz. The system
    /// channel is never changed. History is bounded to one frame plus 350 ms.
    pub fn process(&mut self, stereo: &mut [f32]) {
        let frames = stereo.len() / 2;
        if !(80..=1600).contains(&frames) {
            self.reset();
            return;
        }
        let mic: Vec<f32> = stereo
            .as_chunks::<2>()
            .0
            .iter()
            .map(|s| finite(s[0]))
            .collect();
        self.reference
            .extend(stereo.as_chunks::<2>().0.iter().map(|s| finite(s[1])));
        while self.reference.len() > MAX_DELAY + frames {
            self.reference.pop_front();
        }
        let reference = self.reference.make_contiguous();
        let origin = reference.len() - frames;
        let max_delay = MAX_DELAY.min(origin);
        let mut best = (0.0f64, 0usize, 0.0f64);
        // Coarse search is cheap enough for live capture; refine around the
        // strongest match at full sample resolution before subtracting anything.
        // Average before decimating so a sub-stride delay cannot alias away
        // the match (raw strided samples miss even a one-sample offset).
        let low_mic: Vec<f32> = mic
            .as_chunks::<STRIDE>()
            .0
            .iter()
            .map(|chunk| chunk.iter().sum::<f32>() / STRIDE as f32)
            .collect();
        let low_reference: Vec<f32> = reference
            .as_chunks::<STRIDE>()
            .0
            .iter()
            .map(|chunk| chunk.iter().sum::<f32>() / STRIDE as f32)
            .collect();
        for delay in (0..=max_delay).step_by(STRIDE) {
            let start = (origin - delay) / STRIDE;
            let (score, gain) =
                correlation(&low_mic, &low_reference[start..start + low_mic.len()], 1);
            if score > best.0 {
                best = (score, delay, gain);
            }
        }
        if best.0 < 0.30 {
            return;
        }
        let coarse = best.1;
        best.0 = 0.0;
        for delay in coarse.saturating_sub(STRIDE * 2)..=(coarse + STRIDE * 2).min(max_delay) {
            let (score, gain) =
                correlation(&mic, &reference[origin - delay..origin - delay + frames], 1);
            if score > best.0 {
                best = (score, delay, gain);
            }
        }
        // Strong waveform agreement is required, not just similar loudness.
        // Subtract only the correlated component, preserving double-talk.
        if best.0 < 0.85 || best.2.abs() > 2.0 {
            return;
        }
        let matched = &reference[origin - best.1..origin - best.1 + frames];
        let mean = matched.iter().map(|s| f64::from(*s)).sum::<f64>() / frames as f64;
        for (frame, sample) in stereo.as_chunks_mut::<2>().0.iter_mut().zip(matched) {
            frame[0] = (f64::from(finite(frame[0])) - best.2 * (f64::from(*sample) - mean)) as f32;
        }
    }
}

fn finite(sample: f32) -> f32 {
    if sample.is_finite() { sample } else { 0.0 }
}

fn correlation(mic: &[f32], reference: &[f32], stride: usize) -> (f64, f64) {
    let count = mic.len().div_ceil(stride) as f64;
    let (mut a, mut b, mut aa, mut bb, mut ab) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for (m, r) in mic.iter().zip(reference).step_by(stride) {
        let (m, r) = (f64::from(*m), f64::from(*r));
        a += m;
        b += r;
        aa += m * m;
        bb += r * r;
        ab += m * r;
    }
    let mic_energy = (aa - a * a / count).max(0.0);
    let ref_energy = (bb - b * b / count).max(0.0);
    if mic_energy < count * 0.000_001 || ref_energy < count * 0.000_001 {
        return (0.0, 0.0);
    }
    let cross = ab - a * b / count;
    (
        (cross / (mic_energy * ref_energy).sqrt()).abs(),
        cross / ref_energy,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn signal(n: usize, seed: u32) -> Vec<f32> {
        let mut state = seed;
        (0..n)
            .map(|_| {
                state = state.wrapping_mul(1664525).wrapping_add(1013904223);
                ((state >> 8) as f32 / 16777216.0 - 0.5) * 0.3
            })
            .collect()
    }

    #[test]
    fn removes_delayed_playback_but_preserves_system_channel_and_double_talk() {
        let playback = signal(16000, 7);
        let voice = signal(16000, 53);
        for delay in [0, 137, 1237, 4000] {
            let mut filter = EchoSuppressor::default();
            let (mut error, mut expected_energy) = (0.0f64, 0.0f64);
            for start in (0usize..16000).step_by(800) {
                let mut stereo = Vec::new();
                for i in start..start + 800 {
                    let echo = i.checked_sub(delay).map_or(0.0, |j| playback[j] * 0.7);
                    stereo.extend([echo + voice[i] * 0.12, playback[i]]);
                }
                filter.process(&mut stereo);
                for (i, frame) in stereo.as_chunks::<2>().0.iter().enumerate() {
                    assert_eq!(frame[1], playback[start + i]);
                    if start > delay + 800 {
                        error += f64::from((frame[0] - voice[start + i] * 0.12).powi(2));
                        expected_energy += f64::from((voice[start + i] * 0.12).powi(2));
                    }
                }
                assert!(filter.reference.len() <= MAX_DELAY + 800);
            }
            assert!(
                error < expected_energy * 0.08,
                "delay={delay}, residual error {error}/{expected_energy}"
            );
        }
    }

    #[test]
    fn unrelated_speech_and_mic_only_audio_remain_unchanged() {
        let mic = signal(800, 17);
        for remote in [signal(800, 39), vec![0.0; 800]] {
            let original: Vec<f32> = mic.iter().zip(remote).flat_map(|(m, r)| [*m, r]).collect();
            let mut stereo = original.clone();
            EchoSuppressor::default().process(&mut stereo);
            assert_eq!(stereo, original);
        }
    }
}
