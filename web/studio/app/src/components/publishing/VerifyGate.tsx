// 본인인증 전 — 저작권 관리 신청·계약은 실명으로만 한다. 안내 카드 아래 큰 버튼으로 이 화면에서 바로 인증한다
// (계정 정보로 나갔다 돌아오면 고르던 내용을 다시 찾아야 했다). 한 번 인증하면 계정에 남아 다음부터는 보이지 않는다
import { useState } from 'react';
import { Modal } from '../Modal';
import { IdentityVerify } from '../IdentityVerify';
import { useToast } from '../Toast';
import { Callout } from '../../ui';
import '../../styles/split/Publishing.css';

export function VerifyGate({ what }: { what: '신청서' | '계약' }) {
  const toast = useToast();
  const [open, setOpen] = useState(false);
  return (
    <div className="aq-pbl-verify">
      <Callout tone="neutral">
        <b>본인인증이 필요해요.</b> 저작권 관리 {what === '신청서' ? '신청서는' : '계약은'} 본인인증한 실명으로만 {what === '신청서' ? '서명해요' : '맺어요'}. 관리 단체에도 활동명·필명이 아니라 실명으로 등록돼요. 한 번 인증하면 다음부터는 묻지 않아요.
      </Callout>
      <button type="button" className="ui-btn is-primary is-block is-lg" onClick={() => setOpen(true)}>본인인증하기</button>
      {open && (
        <Modal title="본인인증" onClose={() => setOpen(false)} modalClass="aq-set-modal">
          <IdentityVerify onDone={() => { setOpen(false); toast('본인인증을 마쳤어요. 이어서 서명해 주세요.', 'success'); }} />
        </Modal>
      )}
    </div>
  );
}
