Name:           aimanity
Version:        @VERSION@
Release:        1
Summary:        AImanity native Android and Linux workbench
License:        Apache-2.0
URL:            https://github.com/MrMilkDragon/AImanity
BuildArch:      x86_64
AutoReqProv:    no
Requires:       glibc >= 2.36, @LINKED@
Requires:       vulkan-loader, libwayland-client, libX11, libxkbcommon, polkit, espeak-ng, systemd-udev
Requires:       (google-noto-sans-cjk-fonts or google-noto-sans-cjk-vf-fonts)

%global __os_install_post %{nil}
%description
Offline workspace with one Android phone, one Linux desktop and a shared agent.

%install
mkdir -p "%{buildroot}"
cp -al "$AIMANITY_STAGE/usr" "%{buildroot}/"

%post
@ACTIVATE_KVM@

%files
%defattr(-,root,root,-)
/usr/bin/aimanity
/usr/lib/aimanity
/usr/lib/udev/rules.d/70-aimanity-kvm.rules
/usr/share/applications/dev.aimanity.native.desktop
/usr/share/icons/hicolor/scalable/apps/dev.aimanity.native.svg
/usr/share/doc/aimanity
