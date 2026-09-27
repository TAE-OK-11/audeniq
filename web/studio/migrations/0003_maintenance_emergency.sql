-- 긴급 점검: 관리 화면의 '긴급 점검 시작'으로 만든 점검은 kind='emergency',
-- 끝나는 시각을 모르면 end_unknown=1 (스튜디오에 '종료 시각 미정'으로 표시).
ALTER TABLE maintenance ADD COLUMN kind TEXT NOT NULL DEFAULT 'scheduled' CHECK (kind IN ('scheduled', 'emergency'));
ALTER TABLE maintenance ADD COLUMN end_unknown INTEGER NOT NULL DEFAULT 0 CHECK (end_unknown IN (0, 1));
