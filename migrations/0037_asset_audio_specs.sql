-- 0031: ffprobe-measured audio technical specs on assets.
--
-- catalog.assets.{sample_rate,channels,bits_per_sample}: real technical
-- specs measured by Stage 1 (submission.analyze_asset via
-- qc.probe_audio_metrics), persisted alongside duration_secs. The DDEX ERN
-- 3.8.2 builder used to emit fabricated constants (1411 kbps / 44100 Hz /
-- 16-bit / stereo) for every WAV; it now emits these measured values and
-- omits the elements when unknown. NULL for images, for rows probed before
-- this migration, and when probing failed. Only filled when unknown; the
-- submit path never overwrites a measured value.
ALTER TABLE catalog.assets
    ADD COLUMN sample_rate INTEGER
    CHECK (sample_rate IS NULL OR sample_rate > 0),
    ADD COLUMN channels INTEGER
    CHECK (channels IS NULL OR channels > 0),
    ADD COLUMN bits_per_sample INTEGER
    CHECK (bits_per_sample IS NULL OR bits_per_sample > 0);
