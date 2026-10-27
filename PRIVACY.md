# Política de privacidad / Privacy policy

*Última actualización / Last updated: 27 de octubre de 2026 / October 27, 2026*

## Español

Notch Isla **no recopila, no guarda y no envía ningún dato personal**. No tiene telemetría, estadísticas de uso, anuncios ni cuentas.

La única conexión a internet es la de **actualizaciones**: un minuto después de abrirse y luego una vez al día, la app pide a GitHub (`api.github.com`) el número de la última versión publicada. Si hay una versión nueva y la app está instalada, descarga su instalador desde `github.com` a la carpeta temporal; solo se instala si haces clic en el aviso. Las peticiones no llevan datos tuyos, solo el nombre y la versión de la app. Como cualquier conexión, GitHub ve tu dirección IP; se aplica la [declaración de privacidad de GitHub](https://docs.github.com/es/site-policy/privacy-policies/github-general-privacy-statement).

Para funcionar, la app usa información que ya está en tu PC, solo en memoria y mientras está abierta:

- **Música que suena**: título, artista, carátula, estado y avance de la canción, que Windows comparte con cualquier app a través de los controles multimedia del sistema (SMTC).
- **Ícono de la app que reproduce** (Spotify, Chrome...): se obtiene de Windows o del archivo `.exe` del programa en ejecución.
- **Volumen del sistema**: solo se lee y se cambia cuando giras la rueda del mouse sobre la isla.

Lo único que la app escribe en tu PC es la opción **Iniciar con Windows**, si la activas: un valor llamado `NotchIsla` en `HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Run`. Se quita al desactivar la opción o al desinstalar la app.

Fuera de las actualizaciones, ningún dato sale de tu equipo ni se comparte con terceros.

Preguntas: abre un *issue* en https://github.com/gportal21/notch-isla/issues

## English

Notch Isla **does not collect, store, or send any personal data**. It has no telemetry, usage statistics, ads, or accounts.

Its only internet connection is for **updates**: one minute after it starts and then once a day, the app asks GitHub (`api.github.com`) for the number of the latest published version. If there is a new version and the app is installed, it downloads the installer from `github.com` to the temporary folder; it is installed only if you click the notification. The requests carry none of your data, only the app's name and version. As with any connection, GitHub sees your IP address; the [GitHub General Privacy Statement](https://docs.github.com/en/site-policy/privacy-policies/github-general-privacy-statement) applies.

To work, the app uses information already on your PC, only in memory and only while it is running:

- **The music that is playing**: title, artist, cover art, playback state, and position, which Windows shares with any app through the system media transport controls (SMTC).
- **The icon of the app that is playing** (Spotify, Chrome...): obtained from Windows or from the running program's `.exe` file.
- **System volume**: read and changed only when you turn the mouse wheel over the island.

The only thing the app writes to your PC is the **Start with Windows** option, if you enable it: a value named `NotchIsla` under `HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Run`. It is removed when you turn the option off or uninstall the app.

Apart from updates, no data leaves your computer or is shared with third parties.

Questions: open an issue at https://github.com/gportal21/notch-isla/issues
