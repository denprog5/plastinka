; Установщик Пластинки (Inno Setup 6). Собирается скриптом scripts\build-installer.ps1.
; Ставится в профиль пользователя без прав администратора. Один и тот же файл
; Plastinka-Setup-<версия>.exe — и для первой установки, и для автообновления
; (тогда он запускается с /VERYSILENT /RELAUNCH=1).

#ifndef AppVersion
  #define AppVersion "1.0.1"
#endif
#ifndef BinDir
  #define BinDir "..\target\release"
#endif

#define AppName "Пластинка"
#define AppExe "plastinka.exe"
#define AppIdGuid "{4902DA2C-E42E-48A7-A1EC-ECDA8878AAA4}"
#define RunKey "Software\Microsoft\Windows\CurrentVersion\Run"

[Setup]
; AppId не менять: по нему обновление находит установленную программу.
AppId={{#AppIdGuid}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher=denprog5
AppPublisherURL=https://github.com/denprog5/plastinka
AppSupportURL=https://github.com/denprog5/plastinka/issues
AppUpdatesURL=https://github.com/denprog5/plastinka/releases
VersionInfoVersion={#AppVersion}
DefaultDirName={autopf}\Plastinka
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0.17763
OutputDir=..\dist
OutputBaseFilename=Plastinka-Setup-{#AppVersion}
SetupIconFile=..\assets\icon.ico
WizardSmallImageFile=..\assets\wizard-small.png,..\assets\wizard-small-2x.png
UninstallDisplayIcon={app}\{#AppExe}
UninstallDisplayName={#AppName}
Compression=lzma2/ultra64
SolidCompression=yes
WizardStyle=modern
; Приложение висит в трее: перед обновлением файлов его нужно закрыть.
CloseApplications=force
RestartApplications=no

[Languages]
Name: "ru"; MessagesFile: "compiler:Languages\Russian.isl"

[Tasks]
Name: "autostart"; Description: "Запускать вместе с Windows"; GroupDescription: "Дополнительно:"
Name: "desktopicon"; Description: "Ярлык на рабочем столе"; GroupDescription: "Дополнительно:"; Flags: unchecked

[Files]
Source: "{#BinDir}\{#AppExe}"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExe}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; Tasks: desktopicon

[Registry]
; Автозапуск дальше переключается из меню трея. Тихое обновление его не трогает,
; чтобы не вернуть то, что пользователь выключил.
Root: HKCU; Subkey: "{#RunKey}"; ValueType: string; ValueName: "Plastinka"; ValueData: """{app}\{#AppExe}"""; Tasks: autostart; Check: not WizardSilent

[Run]
Filename: "{app}\{#AppExe}"; Description: "Запустить {#AppName}"; Flags: nowait postinstall skipifsilent
; Автообновление запускает установщик с /RELAUNCH=1 — после него программа стартует снова.
Filename: "{app}\{#AppExe}"; Flags: nowait; Check: IsRelaunch

[UninstallRun]
Filename: "{sys}\taskkill.exe"; Parameters: "/F /IM {#AppExe}"; Flags: runhidden; RunOnceId: "KillApp"

[Code]
function IsRelaunch: Boolean;
begin
  Result := WizardSilent and (ExpandConstant('{param:relaunch|0}') = '1');
end;

// Закрыть работающий экземпляр до копирования файлов (Restart Manager не всегда закрывает трей-приложения).
function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  Code: Integer;
begin
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/F /IM {#AppExe}', '', SW_HIDE, ewWaitUntilTerminated, Code);
  Result := '';
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  Dir: String;
begin
  if CurUninstallStep = usUninstall then
    // Автозапуск мог включить и установщик, и меню трея.
    RegDeleteValue(HKCU, '{#RunKey}', 'Plastinka');
  if CurUninstallStep = usPostUninstall then
  begin
    // В %APPDATA%\Plastinka только config.json (порт и галочки меню). При тихом удалении сохраняем.
    Dir := ExpandConstant('{userappdata}\Plastinka');
    if DirExists(Dir) and not UninstallSilent then
      DelTree(Dir, True, True, True);
  end;
end;
