package chat.app.directory

import io.ktor.client.HttpClient
import io.ktor.client.call.body
import io.ktor.client.engine.cio.CIO
import io.ktor.client.plugins.ClientRequestException
import io.ktor.client.plugins.ServerResponseException
import io.ktor.client.plugins.contentnegotiation.ContentNegotiation
import chat.engine.BackupBundle
import io.ktor.client.request.bearerAuth
import io.ktor.client.request.get
import io.ktor.client.request.parameter
import io.ktor.client.request.post
import io.ktor.client.request.put
import io.ktor.client.request.setBody
import io.ktor.client.statement.HttpResponse
import io.ktor.http.ContentType
import io.ktor.http.HttpStatusCode
import io.ktor.http.contentType
import io.ktor.serialization.kotlinx.json.json
import kotlinx.serialization.Serializable

/** Thrown for any non-2xx directory response, carrying the server's `error` message when present. */
class DirectoryException(message: String) : Exception(message)

@Serializable
data class VerifyResult(val userId: Long, val sessionToken: String, val verified: Boolean)

/** [searchHash] is what the caller compares locally against its own computed hashes — the server never picks the match (T3/T17). */
@Serializable
data class SearchResult(val userId: Long, val handle: String, val searchHash: String)

@Serializable
private data class ErrorBody(val error: String)

@Serializable
private data class SignupRequest(val phone: String)

@Serializable
private data class VerifyRequest(val phone: String, val code: String)

@Serializable
private data class VerifyResponse(val user_id: Long, val session_token: String, val verified: Boolean)

@Serializable
private data class UsernameRequest(val nickname: String)

@Serializable
private data class UsernameResponse(val handle: String)

@Serializable
private data class UsernameLookupResponse(val user_id: Long)

@Serializable
private data class SearchableRequest(val searchable: Boolean, val phone_search_hash: String? = null)

@Serializable
private data class SearchResponse(val results: List<SearchResultDto>)

@Serializable
private data class SearchResultDto(val user_id: Long, val handle: String, val search_hash: String)

@Serializable
private data class BackupPutRequest(
    val blob: String,
    val w_pin: String,
    val salt_p: String,
    val w_phrase: String,
    val salt_f: String,
    val auth_pin: String,
    val salt_a: String,
    val auth_phrase: String,
    val salt_pa: String,
)

@Serializable
private data class PairingBootstrapRequest(val contact_link_b64: String)

@Serializable
private data class PairingBootstrapResponse(val contact_link_b64: String)

/**
 * `directory`'s `verify::normalize_e164` requires a bare "+<8-15 digits>",
 * no spaces/dashes — strips everything else so a phone field the user typed
 * with formatting still reaches the server in the shape it expects.
 */
fun normalizePhoneInput(raw: String): String {
    val trimmed = raw.trim()
    val sign = if (trimmed.startsWith("+")) "+" else ""
    return sign + trimmed.filter { it.isDigit() }
}

/**
 * T27 onboarding: thin wrapper over directory's REST API
 * (directory/src/api.rs) — signup, verify, and claim a username. Nothing
 * else in the app can construct a session token; this is the only path
 * that mints one.
 */
class DirectoryClient(baseUrl: String = defaultDirectoryBaseUrl()) {
    private val baseUrl = baseUrl.trimEnd('/')

    private val http = HttpClient(CIO) {
        install(ContentNegotiation) { json() }
    }

    /** POST /signup — requests an OTP be sent to `phone`. */
    suspend fun signup(phone: String) {
        call { http.post("$baseUrl/signup") {
            contentType(ContentType.Application.Json)
            setBody(SignupRequest(phone))
        } }
    }

    /** POST /verify — checks the OTP and returns a session token. */
    suspend fun verify(phone: String, code: String): VerifyResult {
        val res: VerifyResponse = call { http.post("$baseUrl/verify") {
            contentType(ContentType.Application.Json)
            setBody(VerifyRequest(phone, code))
        } }.body()
        return VerifyResult(res.user_id, res.session_token, res.verified)
    }

    /** POST /username — claims a nickname, returns the rendered handle (e.g. "mira#07"). */
    suspend fun claimUsername(sessionToken: String, nickname: String): String {
        val res: UsernameResponse = call { http.post("$baseUrl/username") {
            bearerAuth(sessionToken)
            contentType(ContentType.Application.Json)
            setBody(UsernameRequest(nickname))
        } }.body()
        return res.handle
    }

    /** GET /username-lookup — resolves a public handle to a user_id, or null if unclaimed. OQ10's default discovery path. */
    suspend fun lookupUsername(sessionToken: String, nickname: String, discriminator: Int): Long? {
        val res: UsernameLookupResponse? = callOrNull { http.get("$baseUrl/username-lookup") {
            bearerAuth(sessionToken)
            parameter("nickname", nickname)
            parameter("discriminator", discriminator)
        } }?.body()
        return res?.user_id
    }

    /**
     * POST /searchable — opt in or out of phone-number search. Opting in
     * requires [phoneSearchHash] for the number verified at signup: an
     * unkeyed SHA-256, computed here, never the server's secret-peppered
     * auth hash (OQ4) — see [phoneSearchHash].
     */
    suspend fun setSearchable(sessionToken: String, searchable: Boolean, phoneSearchHash: String? = null) {
        call { http.post("$baseUrl/searchable") {
            bearerAuth(sessionToken)
            contentType(ContentType.Application.Json)
            setBody(SearchableRequest(searchable, phoneSearchHash))
        } }
    }

    /** GET /search — the k-anonymity bucket for a 5-hex-char prefix; caller matches locally against known contacts' hashes. */
    suspend fun search(sessionToken: String, prefix: String): List<SearchResult> {
        val res: SearchResponse = call { http.get("$baseUrl/search") {
            bearerAuth(sessionToken)
            parameter("prefix", prefix)
        } }.body()
        return res.results.map { SearchResult(it.user_id, it.handle, it.search_hash) }
    }

    /**
     * PUT /v1/backup — upload the E2E-encrypted recovery bundle (issue #2).
     * Every field is ciphertext, a salt, or an auth hash straight from the
     * engine's `BackupBundle`; the server can never read the blob.
     */
    @OptIn(kotlin.io.encoding.ExperimentalEncodingApi::class)
    suspend fun putBackup(sessionToken: String, bundle: BackupBundle) {
        val b64 = kotlin.io.encoding.Base64.Default
        call { http.put("$baseUrl/v1/backup") {
            bearerAuth(sessionToken)
            contentType(ContentType.Application.Json)
            setBody(BackupPutRequest(
                blob = b64.encode(bundle.blob),
                w_pin = b64.encode(bundle.wPin),
                salt_p = b64.encode(bundle.saltP),
                w_phrase = b64.encode(bundle.wPhrase),
                salt_f = b64.encode(bundle.saltF),
                auth_pin = b64.encode(bundle.authPin),
                salt_a = b64.encode(bundle.saltA),
                auth_phrase = b64.encode(bundle.authPhrase),
                salt_pa = b64.encode(bundle.saltPa),
            ))
        } }
    }

    /** POST /pairing-bootstrap — publish (or replenish) this account's one-time contact link for search-initiated pairing. */
    suspend fun publishPairingBootstrap(sessionToken: String, contactLinkB64: String) {
        call { http.post("$baseUrl/pairing-bootstrap") {
            bearerAuth(sessionToken)
            contentType(ContentType.Application.Json)
            setBody(PairingBootstrapRequest(contactLinkB64))
        } }
    }

    /** POST /pairing-bootstrap/request — consumes a target's one-time contact link, or null if none is published. */
    suspend fun requestPairingBootstrap(sessionToken: String, targetUserId: Long): String? {
        val res: PairingBootstrapResponse? = callOrNull { http.post("$baseUrl/pairing-bootstrap/request") {
            bearerAuth(sessionToken)
            parameter("user_id", targetUserId)
        } }?.body()
        return res?.contact_link_b64
    }

    /** Runs a Ktor request, mapping non-2xx statuses to a [DirectoryException] with the server's `error` message. */
    private suspend fun call(request: suspend () -> HttpResponse): HttpResponse {
        try {
            return request()
        } catch (e: ClientRequestException) {
            throw DirectoryException(e.response.errorMessage())
        } catch (e: ServerResponseException) {
            throw DirectoryException(e.response.errorMessage())
        }
    }

    /** Like [call], but a 404 becomes null instead of a thrown exception. */
    private suspend fun callOrNull(request: suspend () -> HttpResponse): HttpResponse? {
        try {
            return request()
        } catch (e: ClientRequestException) {
            if (e.response.status == HttpStatusCode.NotFound) return null
            throw DirectoryException(e.response.errorMessage())
        } catch (e: ServerResponseException) {
            throw DirectoryException(e.response.errorMessage())
        }
    }

    private suspend fun HttpResponse.errorMessage(): String =
        try { body<ErrorBody>().error } catch (_: Exception) { "directory request failed" }
}

/** Unkeyed SHA-256 of the normalized phone number — what the client sends to opt into search (see [DirectoryClient.setSearchable]) and to compute a search prefix ([chat.app.directory.hashPrefix]). */
fun phoneSearchHash(rawPhone: String): String = sha256Hex(normalizePhoneInput(rawPhone))

/** Mirrors directory's PREFIX_LEN_HEX (search.rs) — first 5 hex chars of the search hash. */
fun hashPrefix(fullHash: String): String = fullHash.take(5)
