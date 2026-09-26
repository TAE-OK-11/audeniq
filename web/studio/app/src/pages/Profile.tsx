// 아티스트 정보 — 라이브 view-profile 대응
import { useToast } from '../components/Toast';
import { setProfile, useProfile } from '../store/profile';

const COUNTRY_LABEL: Record<string, string> = {
  KR: '대한민국', US: '미국', JP: '일본', OTHER: '기타',
};

export function Profile() {
  const toast = useToast();
  const profile = useProfile();

  const saveProfile = (e: React.FormEvent) => {
    e.preventDefault();
    const form = e.target as HTMLFormElement;
    const data = new FormData(form);
    setProfile({
      name: String(data.get('profileName') || '').trim(),
      email: String(data.get('profileEmail') || '').trim(),
      bio: String(data.get('profileBio') || '').trim(),
      country: String(data.get('profileCountry') || 'KR'),
    });
    toast('프로필을 저장했어요.');
  };

  return (
    <div id="view-profile" className="view">
      <div className="view-title">
        <div>
          <p className="eyebrow">ARTIST</p>
          <h1>아티스트 정보</h1>
          <p>아티스트 프로필을 관리해 보세요.</p>
        </div>
      </div>

      <div className="aq-profile-card">
        <div className="aq-profile-head">
          <div className="profile-avatar aq-profile-avatar" aria-hidden="true">
            {(profile.name || 'A').slice(0, 1)}
          </div>
          <div className="min-0">
            <strong>{profile.name || '아티스트명 미입력'}</strong>
            <p>{profile.email || '연락 이메일 미입력'} · {COUNTRY_LABEL[profile.country] || '대한민국'}</p>
          </div>
        </div>

        <form id="profileForm" onSubmit={saveProfile}>
          <div className="field">
            <label htmlFor="profileName">활동명 <span className="required">*</span></label>
            <input
              id="profileName" name="profileName" maxLength={120} required autoComplete="nickname"
              placeholder="아티스트명" defaultValue={profile.name} key={profile.name}
            />
          </div>
          <div className="field">
            <label htmlFor="profileEmail">연락 이메일</label>
            <input
              id="profileEmail" name="profileEmail" type="email" maxLength={180} autoComplete="email"
              placeholder="hello@example.com" defaultValue={profile.email} key={profile.email}
            />
          </div>
          <div className="field">
            <label htmlFor="profileBio">아티스트 소개</label>
            <textarea
              id="profileBio" name="profileBio" rows={4} maxLength={1500}
              placeholder="아티스트를 소개해 주세요."
              defaultValue={profile.bio} key={profile.bio}
            />
          </div>
          <div className="field">
            <label htmlFor="profileCountry">활동 국가</label>
            <select id="profileCountry" name="profileCountry" defaultValue={profile.country} key={profile.country}>
              <option value="KR">대한민국</option>
              <option value="US">미국</option>
              <option value="JP">일본</option>
              <option value="OTHER">기타</option>
            </select>
          </div>
          <button type="submit" className="button studio-submit-wide">프로필 저장</button>
        </form>
      </div>
    </div>
  );
}
