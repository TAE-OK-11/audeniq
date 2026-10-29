// 이 기기에서 마지막으로 로그인·가입한 이메일 — 다음에 로그인 화면에 오면 채워 두고 비밀번호 칸부터 (비밀번호는 남기지 않는다)
import { readJSON, writeJSON } from './storage';

const KEY = 'login.lastEmail';
export const lastLoginEmail = () => readJSON<string>(KEY, '');
export function rememberLoginEmail(email: string) { writeJSON(KEY, email); }
