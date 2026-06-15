import { invoke } from "@tauri-apps/api/core";

export type SigningCertSource = "pkcs11" | "win_my";

export type SigningPinUi = "required_in_app" | "hidden_use_os_crypto" | "os_may_prompt";

/** Dónde está la clave privada en almacén Windows MY (solo `win_my`). */
export type WinMyKeyBinding = "smart_card" | "software" | "unknown";

export type SigningCertSummary = {
	id_hex: string;
	label: string;
	subject_dn: string;
	source?: SigningCertSource;
	pin_ui?: SigningPinUi;
	/** Huella SHA-1 del DER; el backend oculta MY duplicado cuando coincide con chip. */
	cert_thumbprint_sha1_hex?: string;
	win_my_key_binding?: WinMyKeyBinding;
};

/** PKCS#11 requiere PIN en la app; almacén MY solo si `required_in_app`. */
export function pinRequiredInApp(cert: SigningCertSummary | null): boolean {
	if (!cert) return true;
	if (cert.source === "win_my") return cert.pin_ui === "required_in_app";
	return true;
}

export type SessionStatusDto = {
	logged_in: boolean;
	/** Compatibilidad; ya no hay cierre por inactividad (valor ignorado en UI). */
	idle_timeout_secs: number;
	seconds_until_auto_logout: number | null;
};

export type Pkcs11SlotDetail = {
	slot_id: number;
	slot_description: string;
	manufacturer_id: string;
	token_present_in_slot_info: boolean;
	token_label: string | null;
};

export type Pkcs11Diagnostics = {
	module_path: string;
	count_pkcs11_get_slot_list_true: number;
	count_effective_for_nexosign: number;
	slots: Pkcs11SlotDetail[];
};

export type Pkcs11ProbeSlotListing = {
	slot_id: number;
	token_label: string | null;
	raw_x509_count: number;
	signing_after_filter_count: number;
	session_error: string | null;
};

export type Pkcs11ProbeModuleListing = {
	path: string;
	slots_with_token: number;
	slots: Pkcs11ProbeSlotListing[];
	error: string | null;
};

export type Pkcs11ProbeCertificateListing = {
	modules: Pkcs11ProbeModuleListing[];
};

export async function probePkcs11ModulePath(): Promise<string> {
	return invoke<string>("probe_pkcs11_module_path");
}

export async function pkcs11DiagnoseSlots(): Promise<Pkcs11Diagnostics> {
	return invoke<Pkcs11Diagnostics>("pkcs11_diagnose_slots");
}

export async function pkcs11ProbeCertificateListing(): Promise<Pkcs11ProbeCertificateListing> {
	return invoke<Pkcs11ProbeCertificateListing>("pkcs11_probe_certificate_listing");
}

export async function pkcs11ListSigningWithPin(pin: string): Promise<SigningCertSummary[]> {
	return invoke<SigningCertSummary[]>("pkcs11_list_signing_with_pin", { pin });
}

export async function pkcs11SlotCount(): Promise<number> {
	return invoke<number>("pkcs11_slot_count");
}

export async function listSigningCertificates(): Promise<SigningCertSummary[]> {
	return invoke<SigningCertSummary[]>("list_signing_certificates");
}

/**
 * Desbloquea el token con el PIN.
 * Si indicas `certIdHex`, la sesión PKCS#11 se abre en el mismo slot que ese certificado (recomendado antes de firmar).
 */
export async function pkcs11Login(pin: string, certIdHex?: string): Promise<void> {
	const payload: Record<string, unknown> = { pin };
	const id = certIdHex?.trim();
	if (id) {
		payload.certIdHex = id;
	}
	return invoke<void>("pkcs11_login", payload);
}

export async function pkcs11Logout(): Promise<void> {
	return invoke<void>("pkcs11_logout");
}

/** Verifica el PIN contra el certificado seleccionado sin dejar sesión abierta. */
export async function pkcs11VerifyPin(pin: string, certIdHex: string): Promise<void> {
	return invoke<void>("pkcs11_verify_pin", { pin, certIdHex });
}

export async function pkcs11ResetConnection(): Promise<void> {
	return invoke<void>("pkcs11_reset_connection");
}

export async function pkcs11SessionStatus(): Promise<SessionStatusDto> {
	return invoke<SessionStatusDto>("pkcs11_session_status");
}
