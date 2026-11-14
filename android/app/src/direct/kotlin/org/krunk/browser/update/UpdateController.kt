package org.krunk.browser.update

import android.annotation.SuppressLint
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.PackageInfo
import android.content.pm.PackageInstaller
import android.content.pm.PackageManager
import android.content.pm.Signature
import android.os.Build
import android.util.Log
import java.io.File
import java.io.IOException
import java.security.MessageDigest
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import org.krunk.browser.BuildConfig
import org.krunk.ffi.AndroidUpdate
import org.krunk.ffi.UpdateFailure
import org.krunk.ffi.UpdateFile
import org.krunk.ffi.UpdateOffer
import org.krunk.ffi.updateCheckDownload
import org.krunk.ffi.updateOffer
import org.krunk.ffi.updatePickFile
import org.krunk.ffi.updateUrls
import org.krunk.ffi.updateVerify

/**
 * The update check of the website APK (direct flavour). Krunk talks to the update server at two
 * moments only, both started by the user: [check] (the manifest and its signature) and
 * [download] (the APK the user confirmed). Nothing runs at start-up, on a timer or in the
 * background; [onAppStart] only removes a file an earlier run left behind.
 *
 * Before the system installer sees an APK: its size and SHA-256 match the signed manifest, and
 * the package manager reads it as Krunk, as the manifest's build, signed with the installed
 * app's certificate. The file lives in app-private storage and is deleted after the hand-over,
 * on any failure and on cancel.
 */
object UpdateController {
    private const val TAG = "KrunkUpdate"
    private const val MAX_MANIFEST = 64 * 1024
    private const val MAX_SIGNATURE = 1024
    const val ACTION_INSTALL_STATUS = "org.krunk.browser.update.INSTALL_STATUS"

    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val _state = MutableStateFlow<UpdateState>(UpdateState.Idle)
    val state: StateFlow<UpdateState> = _state.asStateFlow()
    private var job: Job? = null

    @SuppressLint("StaticFieldLeak") // the application context
    private lateinit var app: Context

    private fun dir(context: Context) = File(context.noBackupFilesDir, "updates")
    private fun apkFile(context: Context) = File(dir(context), "krunk-update.apk")

    /** Process start: delete a downloaded APK and abandon installer sessions left over. No network. */
    fun onAppStart(context: Context) {
        app = context.applicationContext
        scope.launch {
            runCatching { dir(app).listFiles()?.forEach { it.delete() } }
            runCatching {
                val installer = app.packageManager.packageInstaller
                installer.mySessions.forEach { installer.abandonSession(it.sessionId) }
            }
        }
    }

    private fun ensure(context: Context) {
        if (!::app.isInitialized) app = context.applicationContext
    }

    /** Back to the start (after an error or an "up to date" the user has read). */
    fun reset() {
        job?.cancel()
        deleteApk()
        _state.value = UpdateState.Idle
    }

    /** The user tapped "Check for updates": the manifest and its signature, nothing else. */
    fun check(context: Context) {
        ensure(context)
        if (job?.isActive == true) return
        _state.value = UpdateState.Checking
        job = scope.launch {
            _state.value = try {
                evaluate(fetchVerified())
            } catch (e: CancellationException) {
                throw e
            } catch (e: UpdateFailure) {
                Log.w(TAG, "update check refused: $e")
                UpdateState.Failed(UpdateRules.kindOf(e), e.message.orEmpty())
            } catch (e: IOException) {
                Log.w(TAG, "update check failed: $e")
                UpdateState.Failed(UpdateErrorKind.FETCH, e.message.orEmpty())
            }
        }
    }

    private suspend fun fetchVerified(): AndroidUpdate {
        val urls = updateUrls(BuildConfig.UPDATE_BASE_URL, BuildConfig.UPDATE_ALLOW_HTTP)
        val http = UpdateHttp(UpdateRules.userAgent(installedVersionName()))
        val manifest = http.fetchSmall(urls.manifest, MAX_MANIFEST)
        val signature = http.fetchSmall(urls.signature, MAX_SIGNATURE)
        return updateVerify(
            BuildConfig.UPDATE_BASE_URL,
            BuildConfig.UPDATE_ALLOW_HTTP,
            app.packageName,
            manifest,
            signature,
        )
    }

    private fun evaluate(update: AndroidUpdate): UpdateState =
        when (updateOffer(update, installedVersionCode().toULong(), Build.VERSION.SDK_INT.toUInt())) {
            UpdateOffer.UP_TO_DATE -> UpdateState.UpToDate(installedVersionName())

            UpdateOffer.NEEDS_NEWER_ANDROID -> UpdateState.NeedsNewerAndroid(update)

            UpdateOffer.INSTALL -> {
                val file = updatePickFile(update, Build.SUPPORTED_ABIS.toList())
                if (file == null) UpdateState.Failed(UpdateErrorKind.NO_FILE) else UpdateState.Available(update, file)
            }
        }

    /** The user confirmed the download: the one APK, then the checks, then the installer. */
    fun download(context: Context) {
        ensure(context)
        val offer = _state.value as? UpdateState.Available ?: return
        if (job?.isActive == true) return
        val (update, file) = offer
        _state.value = UpdateState.Downloading(update, file, 0)
        job = scope.launch {
            val dest = apkFile(app)
            try {
                dir(app).mkdirs()
                dest.delete()
                val http = UpdateHttp(UpdateRules.userAgent(installedVersionName()))
                val got = http.download(file.url, dest, file.size.toLong()) { bytes ->
                    _state.value = UpdateState.Downloading(update, file, bytes)
                }
                updateCheckDownload(file, got.bytes.toULong(), got.sha256)
                val problem = checkApk(dest, update)
                if (problem != null) {
                    dest.delete()
                    _state.value = UpdateState.Failed(problem)
                    return@launch
                }
                proceed(update)
            } catch (e: CancellationException) {
                dest.delete()
                throw e
            } catch (e: UpdateFailure) {
                Log.w(TAG, "update download refused: $e")
                dest.delete()
                _state.value = UpdateState.Failed(UpdateRules.kindOf(e), e.message.orEmpty())
            } catch (e: IOException) {
                Log.w(TAG, "update download failed: $e")
                dest.delete()
                _state.value = UpdateState.Failed(UpdateErrorKind.DOWNLOAD, e.message.orEmpty())
            }
        }
    }

    /** The user cancelled the download: the partial file goes, the offer stays. */
    fun cancelDownload() {
        val s = _state.value as? UpdateState.Downloading ?: return
        job?.cancel()
        deleteApk()
        _state.value = UpdateState.Available(s.update, s.file)
    }

    /** Krunk may install apps now (the user came back from the system setting). */
    fun permissionMaybeGranted() {
        val s = _state.value as? UpdateState.NeedsPermission ?: return
        scope.launch { proceed(s.update) }
    }

    /** The user declined to allow installs: the checked file is deleted. */
    fun declinePermission() {
        if (_state.value !is UpdateState.NeedsPermission) return
        reset()
    }

    fun canInstall(): Boolean = app.packageManager.canRequestPackageInstalls()

    private fun proceed(update: AndroidUpdate) {
        if (!apkFile(app).isFile) {
            _state.value = UpdateState.Failed(UpdateErrorKind.DOWNLOAD, "file missing")
            return
        }
        if (!canInstall()) {
            _state.value = UpdateState.NeedsPermission(update)
            return
        }
        install(update)
    }

    private fun install(update: AndroidUpdate) {
        val apk = apkFile(app)
        val installer = app.packageManager.packageInstaller
        try {
            val params = PackageInstaller.SessionParams(PackageInstaller.SessionParams.MODE_FULL_INSTALL).apply {
                setAppPackageName(app.packageName)
                setSize(apk.length())
                setInstallReason(PackageManager.INSTALL_REASON_USER)
            }
            val id = installer.createSession(params)
            installer.openSession(id).use { session ->
                session.openWrite("krunk.apk", 0, apk.length()).use { out ->
                    apk.inputStream().use { it.copyTo(out) }
                    session.fsync(out)
                }
                // The session has its own copy now.
                apk.delete()
                val flags = PendingIntent.FLAG_UPDATE_CURRENT or
                    (if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) PendingIntent.FLAG_MUTABLE else 0)
                val intent = Intent(
                    ACTION_INSTALL_STATUS,
                ).setClass(app, InstallReceiver::class.java).setPackage(app.packageName)
                val pending = PendingIntent.getBroadcast(app, id, intent, flags)
                _state.value = UpdateState.Installing(update)
                session.commit(pending.intentSender)
            }
        } catch (e: Exception) {
            Log.w(TAG, "installer failed: $e")
            apk.delete()
            _state.value = UpdateState.Failed(UpdateErrorKind.INSTALLER, e.message.orEmpty())
        }
    }

    /** From [InstallReceiver]: the installer's final answer (success usually ends this process). */
    fun installFinished(status: Int, message: String?) {
        deleteApk()
        _state.value = when (status) {
            PackageInstaller.STATUS_SUCCESS -> UpdateState.UpToDate(installedVersionName())
            PackageInstaller.STATUS_FAILURE_ABORTED -> UpdateState.Failed(UpdateErrorKind.ABORTED)
            else -> UpdateState.Failed(UpdateErrorKind.INSTALLER, message.orEmpty())
        }
    }

    private fun deleteApk() {
        if (::app.isInitialized) runCatching { apkFile(app).delete() }
    }

    // --- what the package manager says ---

    @Suppress("DEPRECATION")
    private fun installedInfo(flags: Int): PackageInfo = app.packageManager.getPackageInfo(app.packageName, flags)

    /**
     * Both flags on every version: before Android 13 the package manager reads an *archive's*
     * certificates only when GET_SIGNATURES is asked for; with GET_SIGNING_CERTIFICATES alone
     * its signingInfo stays empty (seen on API 29: the real update was refused as "wrong
     * certificate"). An empty answer still fails closed in [UpdateRules.checkApk].
     */
    @Suppress("DEPRECATION")
    private fun certFlag(): Int = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
        PackageManager.GET_SIGNING_CERTIFICATES or PackageManager.GET_SIGNATURES
    } else {
        PackageManager.GET_SIGNATURES
    }

    /** From the package manager, not BuildConfig: the QA build's versionCode is set per variant. */
    @Suppress("DEPRECATION")
    fun installedVersionCode(): Long {
        val info = installedInfo(0)
        return if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) info.longVersionCode else info.versionCode.toLong()
    }

    private fun installedVersionName(): String =
        runCatching { installedInfo(0).versionName }.getOrNull() ?: BuildConfig.VERSION_NAME

    @Suppress("DEPRECATION")
    private fun certsOf(info: PackageInfo?): Set<String> {
        if (info == null) return emptySet()
        // signingInfo when the package manager filled it (API 28+), else the older field.
        val fromSigningInfo: Array<Signature>? = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
            info.signingInfo?.apkContentsSigners?.takeIf { it.isNotEmpty() }
        } else {
            null
        }
        val sigs: Array<Signature>? = fromSigningInfo ?: info.signatures
        return sigs.orEmpty().map {
            UpdateRules.hex(MessageDigest.getInstance("SHA-256").digest(it.toByteArray()))
        }.toSet()
    }

    @Suppress("DEPRECATION")
    private fun checkApk(apk: File, update: AndroidUpdate): UpdateErrorKind? {
        val pm = app.packageManager
        val archive = pm.getPackageArchiveInfo(apk.path, certFlag())
        val apkCode = archive?.let {
            if (Build.VERSION.SDK_INT >=
                Build.VERSION_CODES.P
            ) {
                it.longVersionCode
            } else {
                it.versionCode.toLong()
            }
        }
        val result = UpdateRules.checkApk(
            pkg = app.packageName,
            manifestVersionCode = update.versionCode.toLong(),
            manifestCertSha256 = update.signingCertSha256,
            installedCerts = certsOf(installedInfo(certFlag())),
            apkPackage = archive?.packageName,
            apkVersionCode = apkCode,
            apkCerts = certsOf(archive),
        )
        if (result != null) Log.w(TAG, "downloaded APK refused: $result")
        return result
    }

    /** For the UI: the file being offered or downloaded. */
    fun fileOf(s: UpdateState): UpdateFile? = when (s) {
        is UpdateState.Available -> s.file
        is UpdateState.Downloading -> s.file
        else -> null
    }
}
