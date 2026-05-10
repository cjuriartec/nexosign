import { invoke } from "@tauri-apps/api/core";

export type SigningCertSummary = {
	id_hex: string;
	label: string;
	subject_dn: string;
};

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

export async function probePkcs11ModulePath(): Promise<string> {
	return invoke<string>("probe_pkcs11_module_path");
}

export async function pkcs11DiagnoseSlots(): Promise<Pkcs11Diagnostics> {
	return invoke<Pkcs11Diagnostics>("pkcs11_diagnose_slots");
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
