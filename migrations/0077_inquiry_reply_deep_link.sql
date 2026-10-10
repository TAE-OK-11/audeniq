-- 0077: 문의 답변 알림이 해당 문의를 바로 연다 (/inquiries?open=<id>).
-- Same trigger as 0039 (status change + notification on a staff reply); only
-- the notification link now names the inquiry so the studio opens its thread.
CREATE OR REPLACE FUNCTION portal.inquiry_reply_notice() RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog AS $$
BEGIN
 IF NEW.author_kind = 'STAFF' THEN
  UPDATE portal.inquiries SET status = 'ANSWERED', updated_at = now() WHERE id = NEW.inquiry_id;
  INSERT INTO portal.notifications(org_id, user_id, kind, title, detail, link)
  SELECT i.org_id, i.created_by, 'INQUIRY', '문의에 답변이 등록됐어요.', left(i.subject, 150), '/inquiries?open=' || i.id::text
  FROM portal.inquiries i WHERE i.id = NEW.inquiry_id;
 ELSE
  UPDATE portal.inquiries SET status = 'OPEN', updated_at = now() WHERE id = NEW.inquiry_id AND status <> 'CLOSED';
 END IF;
 RETURN NEW;
END $$;
