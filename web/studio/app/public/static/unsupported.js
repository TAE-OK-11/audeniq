// ES 모듈을 지원하지 않는 오래된 브라우저에서만 실행된다 (<script nomodule>). ES5만 쓴다.
(function () {
  var root = document.getElementById('root');
  if (!root) return;
  root.innerHTML =
    '<div class="aq-legacy">' +
    '<strong>브라우저를 업데이트해 주세요</strong>' +
    '<p>지금 브라우저는 AUDENIQ STUDIO를 지원하지 않아요. Chrome·Edge·Safari·Firefox·삼성 인터넷의 최신 버전에서 접속해 주세요.</p>' +
    '</div>';
})();
