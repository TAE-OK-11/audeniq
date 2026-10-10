import type { MeasuredAudio } from './api';

/** Server measurements only; absent values are never guessed from file extensions. */
export function audioSpecs(audio: MeasuredAudio | undefined): string {
  if (!audio) return '분석 정보 없음';
  const positive = (n: number | null) => n !== null && Number.isFinite(n) && n > 0;
  const parts: string[] = [];
  if (positive(audio.duration_secs)) {
    const seconds = Math.round(audio.duration_secs!);
    parts.push(`${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, '0')}`);
  }
  if (positive(audio.sample_rate)) parts.push(`${audio.sample_rate! / 1000} kHz`);
  if (positive(audio.bits_per_sample)) parts.push(`${audio.bits_per_sample} bit`);
  if (positive(audio.channels)) parts.push(`${audio.channels}채널`);
  return parts.join(' · ') || '분석 정보 없음';
}
