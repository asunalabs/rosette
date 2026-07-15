package chat.app.storage

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.platform.LocalContext
import java.io.File
import java.security.GeneralSecurityException
import java.security.KeyStore
import java.security.SecureRandom
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

private const val PREFS = "chat_db_key"
private const val KEY_CT = "ct"
private const val KEY_IV = "iv"
private const val WRAP_ALIAS = "chat_db_key_wrap"

/**
 * Non-exportable AES-GCM wrap key in Android Keystore. No user-auth
 * requirement: the DB key must be available whenever the device is
 * unlocked (default Keystore availability), Signal model.
 */
private fun wrapKey(): SecretKey {
    val ks = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
    (ks.getEntry(WRAP_ALIAS, null) as? KeyStore.SecretKeyEntry)?.let { return it.secretKey }
    val gen = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore")
    gen.init(
        KeyGenParameterSpec.Builder(
            WRAP_ALIAS,
            KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT,
        )
            .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
            .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
            .setKeySize(256)
            .setUserAuthenticationRequired(false)
            .build(),
    )
    return gen.generateKey()
}

private fun getOrCreateDbKey(context: Context): String {
    val prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
    val ct = prefs.getString(KEY_CT, null)
    val iv = prefs.getString(KEY_IV, null)
    if (ct != null && iv != null) {
        try {
            val cipher = Cipher.getInstance("AES/GCM/NoPadding")
            cipher.init(
                Cipher.DECRYPT_MODE,
                wrapKey(),
                GCMParameterSpec(128, Base64.decode(iv, Base64.NO_WRAP)),
            )
            return rawKeyLiteral(cipher.doFinal(Base64.decode(ct, Base64.NO_WRAP)))
        } catch (_: GeneralSecurityException) {
            // Keystore entry rotated/wiped under us (e.g. device-to-device
            // copy): the old DB key is unrecoverable. Fall through to a
            // fresh key — opening the old DB then fails and App.kt's reset
            // dialog handles it. Never a crash, never silent fresh state.
        }
    }
    val key = ByteArray(32).also { SecureRandom().nextBytes(it) }
    val cipher = Cipher.getInstance("AES/GCM/NoPadding")
    cipher.init(Cipher.ENCRYPT_MODE, wrapKey())
    val sealed = cipher.doFinal(key)
    prefs.edit()
        .putString(KEY_CT, Base64.encodeToString(sealed, Base64.NO_WRAP))
        .putString(KEY_IV, Base64.encodeToString(cipher.iv, Base64.NO_WRAP))
        .apply()
    return rawKeyLiteral(key)
}

@Composable
actual fun rememberDbConfig(): DbConfig {
    val context = LocalContext.current.applicationContext
    return remember {
        DbConfig(
            dbPath = File(context.filesDir, "chat.db").absolutePath,
            dbKey = getOrCreateDbKey(context),
        )
    }
}

actual fun deleteDbFile(path: String) {
    File(path).delete()
}
