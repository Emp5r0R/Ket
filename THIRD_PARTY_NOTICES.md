# Third-party notices

## Nginx

The one-command server layout uses the unmodified official Nginx 1.29.5 Alpine image as its HTTPS edge. The multi-architecture image digest is pinned in `compose.edge.yaml`.

Copyright (C) Igor Sysoev; Copyright (C) Nginx, Inc.
All rights reserved.

Redistribution and use in source and binary forms, with or without modification, are permitted provided that the source or binary distribution retains the upstream copyright notice, conditions, and disclaimer. Nginx is provided by its authors and contributors "AS IS", without express or implied warranties. The complete license is included in the image at `/usr/share/licenses/nginx/COPYRIGHT` and at <https://nginx.org/LICENSE>.

## OpenVPN

Ket server and Linux/Windows desktop distributions bundle OpenVPN 2.7.5. OpenVPN is licensed under GNU GPL version 2. Ket uses the unmodified upstream source archive whose version, URL, and SHA-256 digest are recorded in `Dockerfile` and `packaging/fetch-openvpn.sh`.

The complete license is available in the upstream source archive as `COPYING` and at <https://www.gnu.org/licenses/old-licenses/gpl-2.0.txt>. Corresponding source is available from <https://build.openvpn.net/downloads/releases/>.

Ket Android bundles the unmodified OpenVPN 2 native executable and library from OpenVPN for Android 0.7.64 for `armeabi-v7a`, `arm64-v8a`, `x86`, and `x86_64`. The release APK URL and SHA-256 digest are pinned in `packaging/prepare-android-engines.sh`. OpenVPN for Android is distributed under GNU GPL version 2 with the exceptions and additional terms in its `doc/LICENSE.txt`; Ket communicates with the executable only through OpenVPN's management protocol and does not copy its Java class hierarchy. Corresponding source and submodule revisions are available from the `v0.7.64` tag at <https://github.com/schwabe/ics-openvpn>.

## stunnel

Ket server and Linux/Windows desktop distributions bundle stunnel 5.79. stunnel is licensed under GNU GPL version 2 or later with its OpenSSL exception. Ket uses the unmodified upstream source archive whose version, URL, and SHA-256 digest are recorded in `Dockerfile` and `packaging/fetch-stunnel.sh`. The upstream 5.79 release accidentally retained `VERSION_MINOR 78`, so its executable banner reports 5.78 even though the verified 5.79 archive is installed.

The complete license and OpenSSL exception are available in the upstream source archive as `COPYING.md` and `COPYRIGHT.md`. Corresponding source is available from <https://www.stunnel.org/downloads.html>.

## wstunnel

Ket server, desktop, and Android distributions bundle wstunnel 10.6.2 under the BSD 3-Clause license.

Copyright (c) 2020, erebe
All rights reserved.

Redistribution and use in source and binary forms, with or without modification, are permitted provided that the following conditions are met:

1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following disclaimer.
2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the following disclaimer in the documentation and/or other materials provided with the distribution.
3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote products derived from this software without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
