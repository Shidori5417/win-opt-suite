//! Tauri v2 build script with a Windows application manifest requesting
//! administrator elevation (required for HKLM registry, service, scheduled
//! task, power plan, and firewall modifications) while preserving Common
//! Controls v6.

const WINDOWS_APP_MANIFEST: &str = r#"
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <dependency>
    <dependentAssembly>
      <assemblyIdentity
        type="win32"
        name="Microsoft.Windows.Common-Controls"
        version="6.0.0.0"
        processorArchitecture="*"
        publicKeyToken="6595b64144ccf1df"
        language="*"
      />
    </dependentAssembly>
  </dependency>
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="requireAdministrator" uiAccess="false" />
      </requestedPrivileges>
    </security>
  </trustInfo>
</assembly>
"#;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let windows = tauri_build::WindowsAttributes::new().app_manifest(WINDOWS_APP_MANIFEST);
    let attributes = tauri_build::Attributes::new().windows_attributes(windows);
    tauri_build::try_build(attributes)?;
    Ok(())
}
