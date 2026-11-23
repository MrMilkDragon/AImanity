# AImanity 0.0.5 Europium

[English](../../README.md) · [전체 언어](../../README.md) · Apache-2.0

## 다운로드와 현황

[macOS 0.0.5 Europium (DMG)](https://github.com/MrMilkDragon/AImanity/releases/download/v0.0.5/AImanity-0.0.5-macos-arm64.dmg) · [Windows 0.0.5 Europium (EXE)](https://github.com/MrMilkDragon/AImanity/releases/download/v0.0.5/AImanity-0.0.5-windows-x64-setup.exe) · [Linux (DEB)](https://github.com/MrMilkDragon/AImanity/releases/download/v0.0.5/AImanity-0.0.5-linux-amd64.deb) · [Linux (RPM)](https://github.com/MrMilkDragon/AImanity/releases/download/v0.0.5/AImanity-0.0.5-linux-x86_64.rpm)

Apple Silicon이 탑재된 macOS 15.0 이상 또는 Windows 11 x64가 필요합니다. 각 설치 파일에 Android, Linux, 에뮬레이터와 로컬 서비스가 들어 있습니다.

Linux x86_64에는 KVM과 glibc 2.36 이상이 필요합니다. Debian/Ubuntu에서는 DEB, Fedora/Enterprise Linux 10에서는 RPM을 선택하세요. 같은 형식의 새 패키지로 업데이트하면 기기와 대화가 유지됩니다. [설치 명령](release-status.md#linux)

## 기능과 사용 시나리오

AImanity는 Android 휴대폰과 Linux 데스크톱을 하나의 대화와 활성 Agent로 연결합니다. 모델 프로필을 저장하고 설정에서 사용할 프로필을 선택합니다.

- WeChat 같은 앱의 실제 화면과 알림을 확인하고 사용자 지시에 따라 조작하고 답장합니다. 계정과 앱의 제한은 그대로 적용됩니다.
- 휴대폰에서 받은 파일을 Linux로 전송하고 표를 처리·검토하여 돌려보냅니다. 기기 간 전송과 연락처 전달은 별도로 검증합니다.
- 지시를 추가하고 생성을 중단해도 기존 메시지와 작업 기록은 보존합니다. 알림으로 Agent를 깨우며 Liberty는 두 기기에서 작동합니다.
- Skill, 연결된 지식 문서와 작업 파일은 공유합니다. 모델/API, 기억과 외부 연결은 Agent에 속합니다. 기기는 기본적으로 네트워크에 직접 연결합니다. 프록시는 작업 공간에서 설정할 수 있습니다.

작업은 관찰, 기기, 실행, 제어 epoch와 고유 ID에 연결됩니다. 결과가 불명확하면 재실행 전에 관찰합니다.

## 기술과 데이터

`apps/native`: Rust (Iced/wgpu). `crates`: Rust/Axum/Tokio. `packages/protocol`: protobuf. 데이터는 SQLx를 통해 PostgreSQL에 저장합니다. 프로젝트는 Cargo로 빌드합니다.

코드와 사용자 상태는 분리됩니다. 기기 디스크, 계정, 대화, 첨부와 키는 공개하지 않습니다. 업데이트는 버전별 배포물, 백업, 마이그레이션과 롤백을 사용해야 하며 사용자 데이터 위에서 직접 `git pull`하지 않습니다.

## 향후 방향·책임·라이선스

앞으로 음성 서비스 선택지를 늘리고 앱별 스킬과 알림을 받아 시작하는 사무 작업을 확장할 예정입니다.

인터페이스와 초기 설정은 10개 언어를 지원합니다. Skill 왼쪽의 언어 버튼으로 전환하며 게스트 OS와 도구 원문은 자동 번역하지 않습니다. 아랍어는 오른쪽에서 왼쪽으로 표시됩니다.

사용자는 권한, 계정, 외부 작업과 앱 약관에 책임을 집니다. 사기, 기만 또는 동의 없는 작업에 사용하지 마세요. 자체 코드는 Apache-2.0이며 타사 구성 요소는 각 라이선스를 따릅니다.

[출시 현황](release-status.md) · [아키텍처](architecture.en.md) · [호환성](compatibility.md) · [타사 고지](../../THIRD_PARTY_NOTICES.md) · [면책 안내](../../DISCLAIMER.md)
