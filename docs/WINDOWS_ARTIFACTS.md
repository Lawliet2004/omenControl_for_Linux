# Windows supporting-artifact investigation

Only static analysis was performed. No installer, script, service, driver,
assembly, DLL or firmware code from HP/Intel/AMD/Realtek was executed. These
proprietary artifacts are outside the project, under `/tmp/omen-windows-reference/`;
they are not redistributed in source or the Debian package.

## Reference identity

Official HP metadata:
[OMEN Gaming Hub SDK Package sp166157](https://ftp.hp.com/pub/softpaq/sp166001-166500/sp166157.html).
Version **1.0.91 Rev A Pass 1**; listed Windows 11 21H2–25H2 64-bit. The download
came from HP's official `h30318.www3.hp.com` SoftPaq mirror. HP's effective-date
field is December 12, 2025; server timestamps are not treated as a newer product
version or proof that this is the newest Store application.

* Size: 221,403,808 bytes.
* SHA-256: `25e38bb3d94bcf71ef7fec7679ef5100382cb5ce7c14d3522279103d27b4564d`.
* Published-checksum match: MD5 `f981785cde9abfc29b42f9f861a813d3` (an identity
  cross-check, not a substitute for cryptographic signature verification).
* Actual REA `open_binary` result: PE executable, x86_64, same SHA-256.
* Wrapper resource information: hpsoftpaqwrapper.exe, HP Inc.; SDK product
  version 1.0.91. Wrapper file version 0.2.78.55519 is a different version field.
* Existing 7-Zip statically extracted 69 files/21 folders from the wrapper.
  The root-only REA artifact graph did not inventory those embedded contents.

Evidence: `hp-sdk-artifact.json`, `rea-hp-sdk-open.json`,
`rea-hp-sdk-artifact.json`, `hp-sdk-archive-list.txt`, `hp-sdk-file-manifest.json`.
All paths in this document's evidence list are relative to `docs/evidence/`.

## Publisher/signature observation and limits

The wrapper has a PE WIN_CERTIFICATE PKCS#7 table. Static extraction plus existing
OpenSSL identified a signer certificate whose subject is **HP Inc.**, issued by
DigiCert Trusted G4 Code Signing RSA4096 SHA384 2021 CA1. The signer certificate's
validity is February 7, 2025–February 6, 2026. Root/intermediate certificate details
are retained in `hp-sdk-signature-certificates.txt`.

This records embedded certificate data only. Authenticode image-digest validation,
trusted-chain policy, revocation and trusted timestamp verification were **not**
performed. Certificate presence or a historical validity interval alone establishes
neither a valid current signature nor an invalid historical timestamped signature.
Do not describe this artifact as fully signature-verified.

## Components and relationships established statically

| Observation | Evidence and interpretation |
|---|---|
| Multiple installer stages | `PBDConfig.xml` names AMD SDK, Intel XTU, OMENCap, Realtek SDK and HyperX SDK completion flags. `Install.cmd` coordinates installation/logging; its commands were read, never run |
| HP capability driver and service | Nested `CapDriver/OMENCap/1.0.836.0.zip`; packaged `hpomencustomcapcomp.inf` declares service **HPOmenCap**, display **HP Omen HSA Service**, executable `x64/OmenCap/OmenCap.exe`. Base INF declares **HPOmenCustomCapDriver** and `hpomencustomcapdriver.sys`. This is an installation declaration, not observation of a running Windows service |
| Native OmenCap | Existing objdump reports x64 PE, absent CLR directory. Actual REA managed inspection classifies it **not-managed**. It imports Windows service-manager and RPCRT4 server APIs, including RpcServerListen/UseProtseqEp/RegisterIf3. Imports establish API dependencies, not a traced IPC endpoint or main-app protocol |
| Separate capability modules | OmenCap ZIP contains HIDService, RealtekNetworkingService, AmdRyzenOverclockingService, IntelXTUOverclockingService, Device, Platform, ProfileHelperModel and RpcClient.Native libraries. Names suggest responsibilities but do not prove hardware command semantics |
| HP system-capability layer | Nested SysCap version 1.82.4285.0 includes SysInfoCap, WMIService, PowerManagerService and other modules. It cannot be assumed that all are used for a particular OMEN feature/model |
| Vendor driver dependencies | SDK contains Intel iocbios2/ICCWDT drivers and Realtek rtf64x64 network service driver declarations. These Windows drivers are not replaced by raw Linux EC/MSR/network writes |
| Managed components are mixed with native components | REA identifies the assemblies below as .NET Framework/CIL. This does not establish the toolkit of the main OMEN Gaming Hub UI |

Nested capability-file hashes: `hp-sdk-capability-manifest.json`. Native import
observation: `hp-omencap-pe-imports.txt`; native identity: `rea-omencap-identity.json`.
A full Windows installation could select only a subset based on its model and OS;
we have not run installation to determine that selection.

## Actual REA managed inspection

| Artifact | Assembly version / classification | SHA-256 |
|---|---|---|
| OMENSDK_Ver.exe | OMENSDKVer 1.0.91.0, x64 CIL | `6dd9e66f2edbefb125e90fb4e82aaa82b4228498d6418a25da3b6e77c1293571` |
| XtuService.exe | 7.14.2.14, x64 CIL, entry token 0x0600000a | `bbdbbcbf20dd502aa1bf9dabb4ae223dd1aa4c79abf84e4657eb767157462f02` |
| XtuCoreServer.dll | 7.14.2.14, x64 CIL | `f53c5bb660254abe008e060c0645d458f89eb14590431370232d55d81beb64e2` |
| HardwareAccess.dll | 7.14.2.14, x64 CIL | `846d6e3f531d944bc3d34c7a8e7c28ef1fe8e5c6419b25f6281f1ec651a6524d` |

Evidence is retained in `rea-hp-sdk-managed.json` and the compact
`hp-sdk-managed-summary.json`. These calls inventory metadata without loading a
CLR, resolving dependencies by execution, or decompiling C#.

REA `inspect_managed_members` then inspected XtuService's 13 method definitions,
CIL anchors, call edges and strings. The actual build's module MVID is
`c650b31c-8bf3-4435-a20f-a12048c5da5f`. Methods include `XtuService.Program.Main`
(token 0x0600000a), its driver-state check (0x0600000b), and service OnStart/OnStop
(0x06000003/04). Coordinates are build-local and valid only with the hash/MVID.
In this build, OnStart calls `IronCity.Core.Server.CoreServer.Initialize` at IL offset 11 and `Start` at 40; OnStop calls `Stop` at offset 6. Main calls the driver-state check at offsets 203/215 and `ServiceBase.Run` at 260. These are static call anchors, not observed execution.
Full evidence, including CIL hashes/locations and analysis limitations, is retained
in `rea-xtu-service-members.json`; no proprietary implementation is copied into
the Linux application.

## Scope still unverified

The main Microsoft Store application/version, UI technology, its connection to
HPOmenCap, exact RPC/WMI/ACPI command paths, fan/RGB/MUX behavior, capability
selection, live failure handling, and Windows behavioral traces remain unverified.
No binary name, certificate, import or metadata string authorizes a guessed EC
register or WMI command. The Linux app relies on established kernel interfaces;
this supporting-package investigation supplies evidence and follow-up targets,
not a universal hardware-control specification.
