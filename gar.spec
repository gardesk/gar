Name:           gar
Version:        0.1.0
Release:        1%{?dist}
Summary:        Tiling window manager with smart splits

License:        MIT
URL:            https://github.com/gardesk/gar
Source0:        %{name}-%{version}.tar.gz

BuildRequires:  rust >= 1.75
BuildRequires:  cargo
BuildRequires:  libxcb-devel
BuildRequires:  lua-devel

# Disable debug package
%global debug_package %{nil}

%description
Gar is a tiling window manager for X11 built in Rust. Features smart window
splits, workspace management, and Lua configuration. Part of the gardesk
desktop environment suite.

%prep
%autosetup

%build
export CARGO_TARGET_DIR=target
cargo build --release --workspace

%install
install -Dm755 target/release/gar %{buildroot}%{_bindir}/gar
install -Dm755 target/release/garctl %{buildroot}%{_bindir}/garctl
install -Dm644 gar.desktop %{buildroot}%{_datadir}/xsessions/gar.desktop
install -Dm755 start-gar.sh %{buildroot}%{_bindir}/start-gar
install -Dm755 gar-session.sh %{buildroot}%{_bindir}/gar-session

%files
%{_bindir}/gar
%{_bindir}/garctl
%{_bindir}/start-gar
%{_bindir}/gar-session
%{_datadir}/xsessions/gar.desktop

%changelog
* Fri Jan 17 2025 mfw <espadonne@outlook.com> - 0.1.0-1
- Initial RPM release of gar
- Tiling window manager with smart splits
- Lua configuration support
- Part of gardesk desktop suite
