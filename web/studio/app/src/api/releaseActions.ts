// 발매 후 배급 중단·계약 종료·유통사 이전 요청 (서버 portal.release_actions, 0098)
import { orgPath, req } from './http';

export type ReleaseActionKind = 'TAKEDOWN' | 'TERMINATION' | 'TRANSFER';
export type ReleaseActionStatus = 'REQUESTED' | 'IN_PROGRESS' | 'COMPLETED' | 'REJECTED' | 'CANCELLED';

export interface ReleaseActionTask {
  id: string;
  partner_id: string;
  /** QUEUED · SENT · MANUAL_REQUIRED · FAILED · CONFIRMED */
  status: string;
  confirmed_at: string | null;
}

export interface ReleaseAction {
  id: string;
  kind: ReleaseActionKind;
  source: 'ARTIST' | 'STAFF';
  status: ReleaseActionStatus;
  reason: string;
  partners: string[] | null;
  transfer_to: string | null;
  effective_date: string | null;
  decision_note: string | null;
  completed_at: string | null;
  created_at: string;
  tasks: ReleaseActionTask[];
}

export interface ReleaseActionInput {
  kind: ReleaseActionKind;
  reason: string;
  transfer_to?: string;
  effective_date?: string;
}

export interface TransferPacket {
  title: string;
  upc: string | null;
  transfer_to: string;
  effective_date: string;
  tracks: { disc_number: number; track_number: number; title: string; version: string; artist: string; isrc: string | null }[];
  platforms: { partner_id: string; partner_release_id: string | null; first_delivered_at: string; live_status: string | null }[];
}

const base = (release: string) => orgPath(`/releases/${encodeURIComponent(release)}/actions`);

export async function listReleaseActions(release: string): Promise<ReleaseAction[]> {
  const r = await req<{ actions: ReleaseAction[] }>(base(release));
  return r.actions;
}

export async function requestReleaseAction(release: string, input: ReleaseActionInput): Promise<void> {
  await req(base(release), { method: 'POST', body: input });
}

export async function cancelReleaseAction(release: string, action: string): Promise<void> {
  await req(`${base(release)}/${encodeURIComponent(action)}/cancel`, { method: 'POST', body: {} });
}

export async function transferPacket(release: string, action: string): Promise<TransferPacket> {
  return req<TransferPacket>(`${base(release)}/${encodeURIComponent(action)}/transfer-packet`);
}
