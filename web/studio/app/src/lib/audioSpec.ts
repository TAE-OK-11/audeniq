// 음원 파일 머리(header)를 읽어 배급 규격(무손실 PCM, 44.1kHz 이상, 16bit 이상, 2채널 이하)을 확인한다.
// 파일 전체를 읽지 않고 앞부분만 본다 (수백 MB 원본도 즉시 확인).

export interface AudioSpec {
  container: 'WAV' | 'FLAC';
  sampleRate: number;
  bitDepth: number;
  channels: number;
  /** 초 단위 (알 수 없으면 0) */
  duration: number;
  /** PCM 정수 / 부동소수점 */
  float?: boolean;
}

export interface AudioCheck {
  spec: AudioSpec | null;
  /** 배급할 수 없는 사유 (있으면 업로드 중단) */
  error: string;
  /** 배급은 가능하지만 확인이 필요한 점 */
  warnings: string[];
}

const HEAD_BYTES = 256 * 1024;

const ascii = (v: DataView, at: number, n: number) => {
  let s = '';
  for (let i = 0; i < n && at + i < v.byteLength; i++) s += String.fromCharCode(v.getUint8(at + i));
  return s;
};

function parseWav(v: DataView, fileSize: number): AudioSpec | null {
  const riff = ascii(v, 0, 4);
  if ((riff !== 'RIFF' && riff !== 'RF64') || ascii(v, 8, 4) !== 'WAVE') return null;
  let pos = 12;
  let fmt: Omit<AudioSpec, 'duration'> | null = null;
  let byteRate = 0;
  let dataSize = 0;
  let ds64Data = 0;
  while (pos + 8 <= v.byteLength) {
    const id = ascii(v, pos, 4);
    const size = v.getUint32(pos + 4, true);
    const body = pos + 8;
    if (id === 'ds64' && body + 16 <= v.byteLength) {
      // RF64: 실제 data 크기는 64비트 값으로 따로 기록된다
      ds64Data = v.getUint32(body + 8, true) + v.getUint32(body + 12, true) * 2 ** 32;
    } else if (id === 'fmt ' && body + 16 <= v.byteLength) {
      let format = v.getUint16(body, true);
      const channels = v.getUint16(body + 2, true);
      const sampleRate = v.getUint32(body + 4, true);
      byteRate = v.getUint32(body + 8, true);
      let bitDepth = v.getUint16(body + 14, true);
      if (format === 0xfffe && size >= 40 && body + 26 <= v.byteLength) {
        // WAVE_FORMAT_EXTENSIBLE: 유효 비트와 하위 형식 GUID의 앞 2바이트
        const valid = v.getUint16(body + 18, true);
        if (valid) bitDepth = valid;
        format = v.getUint16(body + 24, true);
      }
      if (format !== 1 && format !== 3) return { container: 'WAV', sampleRate, bitDepth, channels, duration: -1 } as AudioSpec;
      fmt = { container: 'WAV', sampleRate, bitDepth, channels, float: format === 3 };
    } else if (id === 'data') {
      dataSize = riff === 'RF64' && ds64Data ? ds64Data : size;
      // data 크기가 비정상이면(스트리밍 기록 등) 파일 크기로 추정
      if (!dataSize || dataSize === 0xffffffff) dataSize = Math.max(0, fileSize - body);
      break;
    }
    pos = body + size + (size % 2);
    if (size === 0xffffffff) break;
  }
  if (!fmt) return null;
  return { ...fmt, duration: byteRate && dataSize ? dataSize / byteRate : 0 };
}

function parseFlac(v: DataView): AudioSpec | null {
  let start = 0;
  // ID3v2 태그가 앞에 붙은 FLAC
  if (ascii(v, 0, 3) === 'ID3' && v.byteLength > 10) {
    const size = ((v.getUint8(6) & 0x7f) << 21) | ((v.getUint8(7) & 0x7f) << 14) | ((v.getUint8(8) & 0x7f) << 7) | (v.getUint8(9) & 0x7f);
    start = 10 + size;
  }
  if (ascii(v, start, 4) !== 'fLaC' || start + 26 > v.byteLength) return null;
  const si = start + 8; // 첫 메타데이터 블록은 반드시 STREAMINFO
  if ((v.getUint8(start + 4) & 0x7f) !== 0) return null;
  const b = (i: number) => v.getUint8(si + 10 + i);
  const sampleRate = (b(0) << 12) | (b(1) << 4) | (b(2) >> 4);
  const channels = ((b(2) >> 1) & 0x07) + 1;
  const bitDepth = (((b(2) & 0x01) << 4) | (b(3) >> 4)) + 1;
  const totalSamples = (b(3) & 0x0f) * 2 ** 32 + ((b(4) << 24) >>> 0) + (b(5) << 16) + (b(6) << 8) + b(7);
  return { container: 'FLAC', sampleRate, bitDepth, channels, duration: sampleRate && totalSamples ? totalSamples / sampleRate : 0 };
}

/** 헤더 바이트로 규격 판정 (테스트에서 직접 호출) */
export function checkAudioHeader(buf: ArrayBuffer, fileSize = buf.byteLength): AudioCheck {
  const v = new DataView(buf);
  const spec = parseWav(v, fileSize) ?? parseFlac(v);
  if (!spec) {
    return { spec: null, error: 'WAV·FLAC 원본 파일이 아니에요. 확장자만 바꾼 파일(MP3 등)은 배급할 수 없어요.', warnings: [] };
  }
  if (spec.duration < 0) {
    return { spec: null, error: '압축된 WAV예요. 무손실 PCM WAV 또는 FLAC 원본으로 다시 내보내 주세요.', warnings: [] };
  }
  const warnings: string[] = [];
  let error = '';
  if (spec.sampleRate < 44100) error = `샘플레이트가 ${(spec.sampleRate / 1000).toFixed(1)}kHz예요. 44.1kHz 이상으로 내보내 주세요.`;
  else if (spec.sampleRate > 192000) error = '샘플레이트가 너무 높아요. 192kHz 이하로 내보내 주세요.';
  else if (spec.bitDepth < 16) error = `비트 깊이가 ${spec.bitDepth}bit예요. 16bit 이상으로 내보내 주세요.`;
  else if (spec.channels > 2) error = `${spec.channels}채널 파일이에요. 스테레오(2채널)로 내보내 주세요.`;
  else if (spec.duration > 0 && spec.duration < 1) error = '재생 시간이 너무 짧아요. 원본 파일을 확인해 주세요.';
  if (!error) {
    if (spec.channels === 1) warnings.push('모노 파일이에요. 스테레오 마스터가 있다면 스테레오로 올려 주세요.');
    if (spec.duration > 0 && spec.duration < 30) warnings.push('30초 미만 곡은 일부 플랫폼에서 재생 수익이 집계되지 않아요.');
  }
  return { spec, error, warnings };
}

export async function checkAudioFile(file: Blob): Promise<AudioCheck> {
  const buf = await file.slice(0, HEAD_BYTES).arrayBuffer();
  return checkAudioHeader(buf, file.size);
}

export function formatDuration(secs: number): string {
  const s = Math.round(secs);
  return `${String(Math.floor(s / 60)).padStart(2, '0')}:${String(s % 60).padStart(2, '0')}`;
}

/** 화면 표기: "WAV · 24bit · 48kHz · 스테레오" */
export function specLabel(s: AudioSpec): string {
  const khz = s.sampleRate % 1000 ? (s.sampleRate / 1000).toFixed(1) : String(s.sampleRate / 1000);
  return [s.container, `${s.bitDepth}bit${s.float ? ' float' : ''}`, `${khz}kHz`, s.channels === 1 ? '모노' : '스테레오'].join(' · ');
}
