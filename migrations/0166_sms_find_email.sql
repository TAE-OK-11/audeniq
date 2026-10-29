-- 이메일 찾기: 등록한 휴대폰 번호로 받은 인증번호를 확인하면 그 번호의 계정 이메일(일부 가림)을 알려 준다.
ALTER TABLE identity.sms_challenges
 DROP CONSTRAINT sms_challenges_purpose_check,
 ADD CONSTRAINT sms_challenges_purpose_check
   CHECK(purpose IN ('PHONE_REGISTER','PASSWORD_RESET','FIND_EMAIL'));
