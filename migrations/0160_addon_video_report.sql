-- 0160: 뮤직비디오 파일을 AUDENIQ 미디어 엔진(`audeniq-music video probe`,
-- crates/core/src/video.rs)으로 확인한 결과. FFprobe 대신 코덱·해상도·
-- 프레임레이트·소리와 배급 검사(mv-delivery-v1)를 업로드 완료 때 저장한다.
--
-- video_report: 엔진 보고서 전체 (streams, issues, rule, engine).
-- video_checks: 주문·신청서에 보여 줄 요약 {rule, mv_ready, summary, issues}.
-- NULL: 엔진이 읽지 못함(길이·해상도는 0158 열에 그대로).
ALTER TABLE portal.addon_files
  ADD COLUMN video_report jsonb NULL,
  ADD COLUMN video_checks jsonb NULL;
