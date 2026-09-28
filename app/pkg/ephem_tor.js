export class App {
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        AppFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_app_free(ptr, 0);
    }
    /**
     * Applies a code: a full link, `#i=` / `#a=` / `#r=` / `#q=`, or bare base64url.
     * `scanned` = it came from the in-app camera (SAS policy, §10.4).
     * Returns 0 or an error code (also emitted as ERROR).
     * @param {string} text
     * @param {boolean} scanned
     * @returns {number}
     */
    apply_code(text, scanned) {
        const ptr0 = passStringToWasm0(text, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.app_apply_code(this.__wbg_ptr, ptr0, len0, scanned);
        return ret >>> 0;
    }
    /**
     * The chat's self-destruct timer in seconds (0 = off).
     * @returns {number}
     */
    chat_ttl() {
        const ret = wasm.app_chat_ttl(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Leaves the chat or room (a member first tells the owner; GOODBYE on every link), wipes
     * session keys and messages.
     */
    close() {
        wasm.app_close(this.__wbg_ptr);
    }
    /**
     * Whether this tab can use the code (for the tab hand-off, §8.7): an answer to one of our
     * open invites, or a reconnect code for one of our links. Never has side effects.
     * @param {string} text
     * @returns {boolean}
     */
    code_fits(text) {
        const ptr0 = passStringToWasm0(text, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.app_code_fits(this.__wbg_ptr, ptr0, len0);
        return ret !== 0;
    }
    /**
     * `kind | flags << 8` of a code without applying it (0 if it is not a valid code), so the UI
     * can ask before answering an identity-transfer or room invite.
     * @param {string} text
     * @returns {number}
     */
    code_info(text) {
        const ptr0 = passStringToWasm0(text, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.app_code_info(this.__wbg_ptr, ptr0, len0);
        return ret >>> 0;
    }
    /**
     * The user confirmed the SAS of the 1:1 chat or of the link to the room owner (§10.4):
     * marks a contact verified; on an identity transfer (§7.6) it releases the key file
     * (sender) or tells the sender (receiver).
     * @returns {number}
     */
    confirm_sas() {
        const ret = wasm.app_confirm_sas(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Dials a contact's onion (§28.7): the chat opens when the contact's Tor tab accepts.
     * @param {string} peer_hex
     * @returns {number}
     */
    contact_connect(peer_hex) {
        const ptr0 = passStringToWasm0(peer_hex, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.app_contact_connect(this.__wbg_ptr, ptr0, len0);
        return ret >>> 0;
    }
    /**
     * One line per contact: `peer_id_hex \t flags \t nickname \t handle`.
     * @returns {string}
     */
    contacts() {
        let deferred1_0;
        let deferred1_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.app_contacts(retptr, this.__wbg_ptr);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            deferred1_0 = r0;
            deferred1_1 = r1;
            return getStringFromWasm0(r0, r1);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
            wasm.__wbindgen_export5(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * Alice: new 1:1 chat and invite. Emits CODE(1).
     * @param {number} ttl_s
     */
    create_invite(ttl_s) {
        wasm.app_create_invite(this.__wbg_ptr, ttl_s);
    }
    /**
     * T3: a reconnect code for the 1:1 chat or for the link to the room owner (§13). Emits CODE(3).
     * @param {number} ttl_s
     * @returns {number}
     */
    create_resume(ttl_s) {
        const ret = wasm.app_create_resume(this.__wbg_ptr, ttl_s);
        return ret >>> 0;
    }
    /**
     * Creates a room owned by us (everything else is closed). Invite members next.
     */
    create_room() {
        wasm.app_create_room(this.__wbg_ptr);
    }
    /**
     * New device (§7.6): an invite asking another device for its identity. Emits CODE(1).
     * @param {number} ttl_s
     */
    create_transfer_invite(ttl_s) {
        wasm.app_create_transfer_invite(this.__wbg_ptr, ttl_s);
    }
    /**
     * Deletes a message: ours for everyone; someone else's for me only, or for everyone by the
     * room owner (moderation). Returns 0 or -error code.
     * @param {number} sender
     * @param {number} seq
     * @returns {number}
     */
    delete(sender, seq) {
        const ret = wasm.app_delete(this.__wbg_ptr, sender, seq);
        return ret;
    }
    /**
     * Diagnostics (§18) of the 1:1 chat or of the link to the room owner (else the first link).
     * @returns {string}
     */
    diag() {
        let deferred1_0;
        let deferred1_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.app_diag(retptr, this.__wbg_ptr);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            deferred1_0 = r0;
            deferred1_1 = r1;
            return getStringFromWasm0(r0, r1);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
            wasm.__wbindgen_export5(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * Drops network paths without leaving, as a network change would. Diagnostics ("Simulate
     * network loss"): the 1:1 chat's path (both sides then go through T3), or in a room the
     * member's direct links to the other members (they come back by T2 through the owner).
     */
    drop_path() {
        wasm.app_drop_path(this.__wbg_ptr);
    }
    /**
     * Edits our message `seq` with `len` bytes at `text_ptr()`. Returns 0 or -error code.
     * @param {number} seq
     * @param {number} len
     * @returns {number}
     */
    edit(seq, len) {
        const ret = wasm.app_edit(this.__wbg_ptr, seq, len);
        return ret;
    }
    /**
     * Our public addresses as the peers see them (srflx candidates of our codes), one per
     * line as `v4 1.2.3.4` / `v6 2001:db8::1` (§29.2 "what your peer sees").
     * @returns {string}
     */
    exposure() {
        let deferred1_0;
        let deferred1_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.app_exposure(retptr, this.__wbg_ptr);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            deferred1_0 = r0;
            deferred1_1 = r1;
            return getStringFromWasm0(r0, r1);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
            wasm.__wbindgen_export5(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * Display handle (`anon_xxxxxx`), not authentication.
     * @returns {string}
     */
    handle() {
        let deferred1_0;
        let deferred1_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.app_handle(retptr, this.__wbg_ptr);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            deferred1_0 = r0;
            deferred1_1 = r1;
            return getStringFromWasm0(r0, r1);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
            wasm.__wbindgen_export5(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * Label of the saved identity in use, or empty for a temporary identity.
     * @returns {string}
     */
    identity_label() {
        let deferred1_0;
        let deferred1_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.app_identity_label(retptr, this.__wbg_ptr);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            deferred1_0 = r0;
            deferred1_1 = r1;
            return getStringFromWasm0(r0, r1);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
            wasm.__wbindgen_export5(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * If the 1:1 peer calls itself by a verified contact's nickname with a different key, that
     * contact's nickname ("This is not the Alice you verified", §7.5); otherwise empty.
     * @param {string} nick
     * @returns {string}
     */
    impersonates(nick) {
        let deferred2_0;
        let deferred2_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            const ptr0 = passStringToWasm0(nick, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            const len0 = WASM_VECTOR_LEN;
            wasm.app_impersonates(retptr, this.__wbg_ptr, ptr0, len0);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            deferred2_0 = r0;
            deferred2_1 = r1;
            return getStringFromWasm0(r0, r1);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
            wasm.__wbindgen_export5(deferred2_0, deferred2_1, 1);
        }
    }
    /**
     * @returns {boolean}
     */
    in_room() {
        const ret = wasm.app_in_room(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * Whether the 1:1 chat is an identity transfer (§7.6).
     * @returns {boolean}
     */
    is_transfer() {
        const ret = wasm.app_is_transfer(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * Signs in with a key file. Only while no chat is open. Returns 0 or an error code.
     * @param {Uint8Array} blob
     * @param {Uint8Array} pass
     * @returns {number}
     */
    load_identity(blob, pass) {
        const ptr0 = passArray8ToWasm0(blob, wasm.__wbindgen_export);
        const len0 = WASM_VECTOR_LEN;
        var ptr1 = passArray8ToWasm0(pass, wasm.__wbindgen_export);
        var len1 = WASM_VECTOR_LEN;
        const ret = wasm.app_load_identity(this.__wbg_ptr, ptr0, len0, ptr1, len1, addHeapObject(pass));
        return ret >>> 0;
    }
    /**
     * Web Lock name for the current identity (§7.2: one identity per tab).
     * @returns {string}
     */
    lock_name() {
        let deferred1_0;
        let deferred1_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.app_lock_name(retptr, this.__wbg_ptr);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            deferred1_0 = r0;
            deferred1_1 = r1;
            return getStringFromWasm0(r0, r1);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
            wasm.__wbindgen_export5(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * The user has seen the 1:1 peer's messages up to `seq`.
     * @param {number} seq
     */
    mark_read(seq) {
        wasm.app_mark_read(this.__wbg_ptr, seq);
    }
    /**
     * Event side-channel block (layout in [`meta`]).
     * @returns {number}
     */
    meta_ptr() {
        const ret = wasm.app_meta_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Our member index (1:1: our PeerIdx), for message identities `(sender, seq)`.
     * @returns {number}
     */
    my_idx() {
        const ret = wasm.app_my_idx(this.__wbg_ptr);
        return ret;
    }
    /**
     * Our role: 0 owner, 1 member, 2 observer (1:1: member).
     * @returns {number}
     */
    my_role() {
        const ret = wasm.app_my_role(this.__wbg_ptr);
        return ret;
    }
    /**
     * Starts with a fresh temporary identity (§7.2 default).
     */
    constructor() {
        const ret = wasm.app_new();
        this.__wbg_ptr = ret;
        AppFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
    /**
     * Replaces the identity with a new temporary one (sign out). Only while no chat is open.
     * @returns {number}
     */
    new_temporary_identity() {
        const ret = wasm.app_new_temporary_identity(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Our nickname, sent to peers in HELLO and kept in the key file (§7.3).
     * @returns {string}
     */
    nick() {
        let deferred1_0;
        let deferred1_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.app_nick(retptr, this.__wbg_ptr);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            deferred1_0 = r0;
            deferred1_1 = r1;
            return getStringFromWasm0(r0, r1);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
            wasm.__wbindgen_export5(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * Our `.onion` address (empty until the service is up).
     * @returns {string}
     */
    onion() {
        let deferred1_0;
        let deferred1_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.app_onion(retptr, this.__wbg_ptr);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            deferred1_0 = r0;
            deferred1_1 = r1;
            return getStringFromWasm0(r0, r1);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
            wasm.__wbindgen_export5(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * The 1:1 peer as a contact: `flags \t nickname`, or empty if not a contact.
     * @returns {string}
     */
    peer_contact() {
        let deferred1_0;
        let deferred1_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.app_peer_contact(retptr, this.__wbg_ptr);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            deferred1_0 = r0;
            deferred1_1 = r1;
            return getStringFromWasm0(r0, r1);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
            wasm.__wbindgen_export5(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * Reacts to `(sender, seq)` with the emoji written at `text_ptr()` (`len` = 0 removes our
     * reaction). Returns 0 or -error code.
     * @param {number} sender
     * @param {number} seq
     * @param {number} len
     * @returns {number}
     */
    react(sender, seq, len) {
        const ret = wasm.app_react(this.__wbg_ptr, sender, seq, len);
        return ret;
    }
    /**
     * @param {string} peer_hex
     * @returns {number}
     */
    remove_contact(peer_hex) {
        const ptr0 = passStringToWasm0(peer_hex, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.app_remove_contact(this.__wbg_ptr, ptr0, len0);
        return ret >>> 0;
    }
    /**
     * @param {string} peer_hex
     * @param {string} nick
     * @returns {number}
     */
    rename_contact(peer_hex, nick) {
        const ptr0 = passStringToWasm0(peer_hex, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len0 = WASM_VECTOR_LEN;
        const ptr1 = passStringToWasm0(nick, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len1 = WASM_VECTOR_LEN;
        const ret = wasm.app_rename_contact(this.__wbg_ptr, ptr0, len0, ptr1, len1);
        return ret >>> 0;
    }
    /**
     * Re-encrypts the saved identity with the key kept in memory (fresh nonce).
     * @returns {Uint8Array}
     */
    resave_identity() {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.app_resave_identity(retptr, this.__wbg_ptr);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var v1 = getArrayU8FromWasm0(r0, r1).slice();
            wasm.__wbindgen_export5(r0, r1 * 1, 1);
            return v1;
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * In-band ICE restart (§13 T1) of every connected link: diagnostics button, or the network
     * changed (`online`, `navigator.connection` change).
     */
    restart_ice() {
        wasm.app_restart_ice(this.__wbg_ptr);
    }
    /**
     * Member: the user accepted that every member will see its IP address (§29.2):
     * start the links to the other members.
     */
    room_connect() {
        wasm.app_room_connect(this.__wbg_ptr);
    }
    /**
     * `my_idx \t my_role \t owner(0/1) \t version \t confirmed(0/1)`, or empty outside a room.
     * @returns {string}
     */
    room_info() {
        let deferred1_0;
        let deferred1_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.app_room_info(retptr, this.__wbg_ptr);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            deferred1_0 = r0;
            deferred1_1 = r1;
            return getStringFromWasm0(r0, r1);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
            wasm.__wbindgen_export5(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * Owner: an invite for one more member (or read-only observer). Emits CODE(1).
     * @param {boolean} observer
     * @param {number} ttl_s
     * @returns {number}
     */
    room_invite(observer, ttl_s) {
        const ret = wasm.app_room_invite(this.__wbg_ptr, observer, ttl_s);
        return ret >>> 0;
    }
    /**
     * One line per member: `idx \t role(0 owner,1 member,2 observer) \t handle \t link \t nick`,
     * where link is `me`, `connected`, `connecting`, `suspended` or `none`.
     * @returns {string}
     */
    room_members() {
        let deferred1_0;
        let deferred1_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.app_room_members(retptr, this.__wbg_ptr);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            deferred1_0 = r0;
            deferred1_1 = r1;
            return getStringFromWasm0(r0, r1);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
            wasm.__wbindgen_export5(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * Owner: removes a member (it gets a state without itself, then GOODBYE).
     * @param {number} member
     * @returns {number}
     */
    room_remove(member) {
        const ret = wasm.app_room_remove(this.__wbg_ptr, member);
        return ret >>> 0;
    }
    /**
     * Saves the peer of the 1:1 chat as a contact (verified if the SAS was confirmed).
     * @param {string} nick
     * @returns {number}
     */
    save_contact(nick) {
        const ptr0 = passStringToWasm0(nick, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.app_save_contact(this.__wbg_ptr, ptr0, len0);
        return ret >>> 0;
    }
    /**
     * Encrypts the current identity into a key file. `pass` (UTF-8) is wiped on return.
     * Returns the file bytes, or an empty array on error (ERROR event emitted).
     * @param {string} label
     * @param {Uint8Array} pass
     * @returns {Uint8Array}
     */
    save_identity(label, pass) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            const ptr0 = passStringToWasm0(label, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            const len0 = WASM_VECTOR_LEN;
            var ptr1 = passArray8ToWasm0(pass, wasm.__wbindgen_export);
            var len1 = WASM_VECTOR_LEN;
            wasm.app_save_identity(retptr, this.__wbg_ptr, ptr0, len0, ptr1, len1, addHeapObject(pass));
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var v3 = getArrayU8FromWasm0(r0, r1).slice();
            wasm.__wbindgen_export5(r0, r1 * 1, 1);
            return v3;
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * Camera frame buffer for [`scan_rgba`]: at least `len` bytes (allocated once, grown only
     * if a larger camera frame appears).
     * @param {number} len
     * @returns {number}
     */
    scan_buf(len) {
        const ret = wasm.app_scan_buf(this.__wbg_ptr, len);
        return ret >>> 0;
    }
    /**
     * Decodes a QR code from the RGBA frame in the scan buffer; empty string if none.
     * @param {number} width
     * @param {number} height
     * @returns {string}
     */
    scan(width, height) {
        let deferred1_0;
        let deferred1_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.app_scan(retptr, this.__wbg_ptr, width, height);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            deferred1_0 = r0;
            deferred1_1 = r1;
            return getStringFromWasm0(r0, r1);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
            wasm.__wbindgen_export5(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * Sends `len` bytes previously written at `text_ptr()`. `reply_seq` = 0 for no reply,
     * otherwise the quoted message `(reply_sender, reply_seq)`. Returns chat_seq or -error code.
     * @param {number} len
     * @param {number} reply_sender
     * @param {number} reply_seq
     * @returns {number}
     */
    send(len, reply_sender, reply_seq) {
        const ret = wasm.app_send(this.__wbg_ptr, len, reply_sender, reply_seq);
        return ret;
    }
    /**
     * Sets our nickname (≤ 32 bytes); applies from the next chat. Re-save a saved identity after.
     * @param {string} nick
     * @returns {number}
     */
    set_nick(nick) {
        const ptr0 = passStringToWasm0(nick, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.app_set_nick(this.__wbg_ptr, ptr0, len0);
        return ret >>> 0;
    }
    /**
     * `privacy`: 0 LAN-only, 1 default, 2 max connectivity. Applies to the next code.
     * @param {number} privacy
     * @param {boolean} drop_ipv6
     * @param {boolean} read_receipts
     * @param {boolean} typing
     */
    set_prefs(privacy, drop_ipv6, read_receipts, typing) {
        wasm.app_set_prefs(this.__wbg_ptr, privacy, drop_ipv6, read_receipts, typing);
    }
    /**
     * Sets the chat's self-destruct timer (1:1: either person; room: the owner).
     * Returns the notice's chat_seq or -error code.
     * @param {number} ttl_s
     * @returns {number}
     */
    set_ttl(ttl_s) {
        const ret = wasm.app_set_ttl(this.__wbg_ptr, ttl_s);
        return ret;
    }
    /**
     * 1:1 chat or link to the room owner: 0 none, 1 gathering, 2 awaiting answer,
     * 3 connecting, 4 connected, 5 closed, 6 suspended. A room owner: 4.
     * @returns {number}
     */
    state() {
        const ret = wasm.app_state(this.__wbg_ptr);
        return ret;
    }
    /**
     * @returns {number}
     */
    text_cap() {
        const ret = wasm.app_text_cap(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Where JS writes outgoing UTF-8 (`TextEncoder.encodeInto`), `MAX_TEXT` bytes.
     * @returns {number}
     */
    text_ptr() {
        const ret = wasm.app_text_ptr(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Timer (JS calls it every second). `hidden` = `document.hidden`, carried in PING.
     * @param {boolean} hidden
     */
    tick(hidden) {
        wasm.app_tick(this.__wbg_ptr, hidden);
    }
    /**
     * The Tor directory as JSON for IndexedDB (public data; empty until downloaded).
     * @returns {string}
     */
    tor_cache() {
        let deferred1_0;
        let deferred1_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.app_tor_cache(retptr, this.__wbg_ptr);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            deferred1_0 = r0;
            deferred1_1 = r1;
            return getStringFromWasm0(r0, r1);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
            wasm.__wbindgen_export5(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * arti logs to the console at `level` (`"info"`, `"debug"`, …; diagnostics only).
     * @param {string} level
     */
    tor_log(level) {
        const ptr0 = passStringToWasm0(level, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len0 = WASM_VECTOR_LEN;
        wasm.app_tor_log(this.__wbg_ptr, ptr0, len0);
    }
    /**
     * Starts arti over Snowflake, then hosts our onion service (key from the identity seed).
     * Progress arrives as TOR events. `ice`: comma-separated `stun:` URLs; `nat`: the broker's
     * NAT hint (empty = "unknown"); `network_toml`: empty for the real Tor network; `cache`:
     * the directory snapshot of `tor_cache` from an earlier session (warm start), or empty.
     * @param {string} broker
     * @param {string} fingerprint
     * @param {string} ice
     * @param {string} nat
     * @param {string} network_toml
     * @param {string} cache
     * @returns {number}
     */
    tor_start(broker, fingerprint, ice, nat, network_toml, cache) {
        const ptr0 = passStringToWasm0(broker, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len0 = WASM_VECTOR_LEN;
        const ptr1 = passStringToWasm0(fingerprint, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len1 = WASM_VECTOR_LEN;
        const ptr2 = passStringToWasm0(ice, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len2 = WASM_VECTOR_LEN;
        const ptr3 = passStringToWasm0(nat, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len3 = WASM_VECTOR_LEN;
        const ptr4 = passStringToWasm0(network_toml, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len4 = WASM_VECTOR_LEN;
        const ptr5 = passStringToWasm0(cache, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len5 = WASM_VECTOR_LEN;
        const ret = wasm.app_tor_start(this.__wbg_ptr, ptr0, len0, ptr1, len1, ptr2, len2, ptr3, len3, ptr4, len4, ptr5, len5);
        return ret >>> 0;
    }
    /**
     * Bootstrap status line (empty before `tor_start`).
     * @returns {string}
     */
    tor_status() {
        let deferred1_0;
        let deferred1_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.app_tor_status(retptr, this.__wbg_ptr);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            deferred1_0 = r0;
            deferred1_1 = r1;
            return getStringFromWasm0(r0, r1);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
            wasm.__wbindgen_export5(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * @param {boolean} active
     */
    typing(active) {
        wasm.app_typing(this.__wbg_ptr, active);
    }
}
if (Symbol.dispose) App.prototype[Symbol.dispose] = App.prototype.free;

/**
 * SVG (quiet zone 4, black on white) for `text`, or an empty string if it does not fit.
 * @param {string} text
 * @returns {string}
 */
export function qr_svg_path(text) {
    let deferred2_0;
    let deferred2_1;
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        const ptr0 = passStringToWasm0(text, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len0 = WASM_VECTOR_LEN;
        wasm.qr_svg_path(retptr, ptr0, len0);
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        deferred2_0 = r0;
        deferred2_1 = r1;
        return getStringFromWasm0(r0, r1);
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
        wasm.__wbindgen_export5(deferred2_0, deferred2_1, 1);
    }
}
function __wbg_get_imports() {
    const import0 = {
        __proto__: null,
        __wbg___wbindgen_boolean_get_5b446f51afd21013: function(arg0) {
            const v = getObject(arg0);
            const ret = typeof(v) === 'boolean' ? v : undefined;
            return isLikeNone(ret) ? 0xFFFFFF : ret ? 1 : 0;
        },
        __wbg___wbindgen_copy_to_typed_array_88899a52af046901: function(arg0, arg1, arg2) {
            new Uint8Array(getObject(arg2).buffer, getObject(arg2).byteOffset, getObject(arg2).byteLength).set(getArrayU8FromWasm0(arg0, arg1));
        },
        __wbg___wbindgen_debug_string_4687d8d8c2017d52: function(arg0, arg1) {
            const ret = debugString(getObject(arg1));
            const ptr1 = passStringToWasm0(ret, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            const len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        },
        __wbg___wbindgen_is_function_1f9d30630b8b1d3d: function(arg0) {
            const ret = typeof(getObject(arg0)) === 'function';
            return ret;
        },
        __wbg___wbindgen_is_null_e343b7d08827ba72: function(arg0) {
            const ret = getObject(arg0) === null;
            return ret;
        },
        __wbg___wbindgen_is_object_3c45d4f2dde4e749: function(arg0) {
            const val = getObject(arg0);
            const ret = typeof(val) === 'object' && val !== null;
            return ret;
        },
        __wbg___wbindgen_is_string_90b56bc79aad6f6c: function(arg0) {
            const ret = typeof(getObject(arg0)) === 'string';
            return ret;
        },
        __wbg___wbindgen_is_undefined_8865fb403f8fe9d8: function(arg0) {
            const ret = getObject(arg0) === undefined;
            return ret;
        },
        __wbg___wbindgen_number_get_2e0e7dee9f701a71: function(arg0, arg1) {
            const obj = getObject(arg1);
            const ret = typeof(obj) === 'number' ? obj : undefined;
            getDataViewMemory0().setFloat64(arg0 + 8 * 1, isLikeNone(ret) ? 0 : ret, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, !isLikeNone(ret), true);
        },
        __wbg___wbindgen_string_get_0380ccaa2f57f0d9: function(arg0, arg1) {
            const obj = getObject(arg1);
            const ret = typeof(obj) === 'string' ? obj : undefined;
            var ptr1 = isLikeNone(ret) ? 0 : passStringToWasm0(ret, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            var len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        },
        __wbg___wbindgen_throw_41e9ee4f547fc59a: function(arg0, arg1) {
            throw new Error(getStringFromWasm0(arg0, arg1));
        },
        __wbg__wbg_cb_unref_dcc1a90847f04c41: function(arg0) {
            getObject(arg0)._wbg_cb_unref();
        },
        __wbg_bufferedAmount_ffae037ee3ccb36c: function(arg0) {
            const ret = getObject(arg0).bufferedAmount;
            return ret;
        },
        __wbg_call_187d372bd5fdd4aa: function() { return handleError(function (arg0, arg1, arg2) {
            const ret = getObject(arg0).call(getObject(arg1), getObject(arg2));
            return addHeapObject(ret);
        }, arguments); },
        __wbg_clearTimeout_c7bc4c3c7a7774af: function(arg0, arg1) {
            getObject(arg0).clearTimeout(arg1);
        },
        __wbg_close_35c28c04c6854ad8: function(arg0) {
            getObject(arg0).close();
        },
        __wbg_close_de72eca08cf70e9c: function(arg0) {
            getObject(arg0).close();
        },
        __wbg_connectionState_ab1e320e4aaa25d6: function(arg0) {
            const ret = getObject(arg0).connectionState;
            return (__wbindgen_enum_RtcPeerConnectionState.indexOf(ret) + 1 || 7) - 1;
        },
        __wbg_createAnswer_4565390027b78031: function(arg0) {
            const ret = getObject(arg0).createAnswer();
            return addHeapObject(ret);
        },
        __wbg_createDataChannel_83caaab3b3db15e4: function(arg0, arg1, arg2, arg3) {
            const ret = getObject(arg0).createDataChannel(getStringFromWasm0(arg1, arg2), getObject(arg3));
            return addHeapObject(ret);
        },
        __wbg_createOffer_419e0ee0d8a14451: function(arg0) {
            const ret = getObject(arg0).createOffer();
            return addHeapObject(ret);
        },
        __wbg_createOffer_d36fbce190f0f3b8: function(arg0, arg1) {
            const ret = getObject(arg0).createOffer(getObject(arg1));
            return addHeapObject(ret);
        },
        __wbg_crypto_38df2bab126b63dc: function(arg0) {
            const ret = getObject(arg0).crypto;
            return addHeapObject(ret);
        },
        __wbg_data_522f7abc70721269: function(arg0) {
            const ret = getObject(arg0).data;
            return addHeapObject(ret);
        },
        __wbg_ephemEvent_3455b5add6d359df: function(arg0, arg1, arg2, arg3) {
            ephemEvent(arg0 >>> 0, arg1, arg2 >>> 0, arg3 >>> 0);
        },
        __wbg_fetch_b322877f1ed6457d: function(arg0, arg1, arg2, arg3) {
            const ret = getObject(arg0).fetch(getStringFromWasm0(arg1, arg2), getObject(arg3));
            return addHeapObject(ret);
        },
        __wbg_forEach_7ad975c8e42636ed: function(arg0, arg1, arg2) {
            try {
                var state0 = {a: arg1, b: arg2};
                var cb0 = (arg0, arg1) => {
                    const a = state0.a;
                    state0.a = 0;
                    try {
                        return __wasm_bindgen_func_elem_5841(a, state0.b, arg0, arg1);
                    } finally {
                        state0.a = a;
                    }
                };
                getObject(arg0).forEach(cb0);
            } finally {
                state0.a = 0;
            }
        },
        __wbg_getRandomValues_436a51d0629d84e1: function() { return handleError(function (arg0, arg1) {
            globalThis.crypto.getRandomValues(getArrayU8FromWasm0(arg0, arg1));
        }, arguments); },
        __wbg_getRandomValues_a608c4436c19407a: function() { return handleError(function (arg0, arg1) {
            globalThis.crypto.getRandomValues(getArrayU8FromWasm0(arg0, arg1));
        }, arguments); },
        __wbg_getRandomValues_c44a50d8cfdaebeb: function() { return handleError(function (arg0, arg1) {
            getObject(arg0).getRandomValues(getObject(arg1));
        }, arguments); },
        __wbg_getStats_9838d727eec9406e: function(arg0) {
            const ret = getObject(arg0).getStats();
            return addHeapObject(ret);
        },
        __wbg_get_31af05bd4842a84f: function() { return handleError(function (arg0, arg1) {
            const ret = Reflect.get(getObject(arg0), getObject(arg1));
            return addHeapObject(ret);
        }, arguments); },
        __wbg_get_464ae6d03ecb8ac7: function(arg0, arg1) {
            const ret = getObject(arg0).get(getObject(arg1));
            return addHeapObject(ret);
        },
        __wbg_iceConnectionState_dbc63bbbe69109ce: function(arg0) {
            const ret = getObject(arg0).iceConnectionState;
            return (__wbindgen_enum_RtcIceConnectionState.indexOf(ret) + 1 || 8) - 1;
        },
        __wbg_iceGatheringState_f6f601e00490767f: function(arg0) {
            const ret = getObject(arg0).iceGatheringState;
            return (__wbindgen_enum_RtcIceGatheringState.indexOf(ret) + 1 || 4) - 1;
        },
        __wbg_instanceof_ArrayBuffer_a99f175873e5d9b8: function(arg0) {
            let result;
            try {
                result = getObject(arg0) instanceof ArrayBuffer;
            } catch (_) {
                result = false;
            }
            const ret = result;
            return ret;
        },
        __wbg_instanceof_Window_82d71df4eddf88bc: function(arg0) {
            let result;
            try {
                result = getObject(arg0) instanceof Window;
            } catch (_) {
                result = false;
            }
            const ret = result;
            return ret;
        },
        __wbg_length_7f3c00c40364105e: function(arg0) {
            const ret = getObject(arg0).length;
            return ret;
        },
        __wbg_localDescription_0e45f6d82b01d1d4: function(arg0) {
            const ret = getObject(arg0).localDescription;
            return isLikeNone(ret) ? 0 : addHeapObject(ret);
        },
        __wbg_log_17c30ef363c61cf4: function(arg0) {
            console.log(getObject(arg0));
        },
        __wbg_msCrypto_bd5a034af96bcba6: function(arg0) {
            const ret = getObject(arg0).msCrypto;
            return addHeapObject(ret);
        },
        __wbg_new_1dbf7428bba60a42: function(arg0) {
            const ret = new Uint8Array(getObject(arg0));
            return addHeapObject(ret);
        },
        __wbg_new_5502aad30c185fc8: function(arg0, arg1) {
            try {
                var state0 = {a: arg0, b: arg1};
                var cb0 = (arg0, arg1) => {
                    const a = state0.a;
                    state0.a = 0;
                    try {
                        return __wasm_bindgen_func_elem_5841_62(a, state0.b, arg0, arg1);
                    } finally {
                        state0.a = a;
                    }
                };
                const ret = new Promise(cb0);
                return addHeapObject(ret);
            } finally {
                state0.a = 0;
            }
        },
        __wbg_new_617a8cdb8bb1130e: function() {
            const ret = new Object();
            return addHeapObject(ret);
        },
        __wbg_new_ee2291f50781bf1d: function() {
            const ret = new Array();
            return addHeapObject(ret);
        },
        __wbg_new_with_configuration_c6d3ab433d0adb73: function() { return handleError(function (arg0) {
            const ret = new RTCPeerConnection(getObject(arg0));
            return addHeapObject(ret);
        }, arguments); },
        __wbg_new_with_length_3da0ad195f6f63ba: function(arg0) {
            const ret = new Uint8Array(arg0 >>> 0);
            return addHeapObject(ret);
        },
        __wbg_node_84ea875411254db1: function(arg0) {
            const ret = getObject(arg0).node;
            return addHeapObject(ret);
        },
        __wbg_now_aa4ccb83129e9e55: function() {
            const ret = Date.now();
            return ret;
        },
        __wbg_now_c14677e1be024a7d: function() {
            const ret = performance.now();
            return ret;
        },
        __wbg_now_e7c6795a7f81e10f: function(arg0) {
            const ret = getObject(arg0).now();
            return ret;
        },
        __wbg_parse_0fc53dead14b3b42: function() { return handleError(function (arg0, arg1) {
            const ret = JSON.parse(getStringFromWasm0(arg0, arg1));
            return addHeapObject(ret);
        }, arguments); },
        __wbg_performance_3fcf6e32a7e1ed0a: function(arg0) {
            const ret = getObject(arg0).performance;
            return addHeapObject(ret);
        },
        __wbg_process_44c7a14e11e9f69e: function(arg0) {
            const ret = getObject(arg0).process;
            return addHeapObject(ret);
        },
        __wbg_prototypesetcall_bc27214492979395: function(arg0, arg1, arg2) {
            Uint8Array.prototype.set.call(getArrayU8FromWasm0(arg0, arg1), getObject(arg2));
        },
        __wbg_push_2baf45db356cf468: function(arg0, arg1) {
            const ret = getObject(arg0).push(getObject(arg1));
            return ret;
        },
        __wbg_queueMicrotask_9833f9a49df95a49: function(arg0) {
            const ret = getObject(arg0).queueMicrotask;
            return addHeapObject(ret);
        },
        __wbg_queueMicrotask_a72f977e97f23c5f: function(arg0) {
            queueMicrotask(getObject(arg0));
        },
        __wbg_randomFillSync_6c25eac9869eb53c: function() { return handleError(function (arg0, arg1) {
            getObject(arg0).randomFillSync(takeObject(arg1));
        }, arguments); },
        __wbg_readyState_418637f3ca14e818: function(arg0) {
            const ret = getObject(arg0).readyState;
            return (__wbindgen_enum_RtcDataChannelState.indexOf(ret) + 1 || 5) - 1;
        },
        __wbg_require_b4edbdcf3e2a1ef0: function() { return handleError(function () {
            const ret = module.require;
            return addHeapObject(ret);
        }, arguments); },
        __wbg_resolve_0076e10020304ede: function(arg0) {
            const ret = Promise.resolve(getObject(arg0));
            return addHeapObject(ret);
        },
        __wbg_sdp_c6ddd96011b47254: function(arg0, arg1) {
            const ret = getObject(arg1).sdp;
            const ptr1 = passStringToWasm0(ret, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            const len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        },
        __wbg_send_1e2ed35a61efd7fd: function() { return handleError(function (arg0, arg1, arg2) {
            getObject(arg0).send(getArrayU8FromWasm0(arg1, arg2));
        }, arguments); },
        __wbg_setLocalDescription_c44231625d771b55: function(arg0, arg1) {
            const ret = getObject(arg0).setLocalDescription(getObject(arg1));
            return addHeapObject(ret);
        },
        __wbg_setRemoteDescription_a8fe607f1eef503d: function(arg0, arg1) {
            const ret = getObject(arg0).setRemoteDescription(getObject(arg1));
            return addHeapObject(ret);
        },
        __wbg_setTimeout_db7bbc18a17e152a: function() { return handleError(function (arg0, arg1, arg2) {
            const ret = getObject(arg0).setTimeout(getObject(arg1), arg2);
            return ret;
        }, arguments); },
        __wbg_set_145a351398b48c65: function() { return handleError(function (arg0, arg1, arg2) {
            const ret = Reflect.set(getObject(arg0), getObject(arg1), getObject(arg2));
            return ret;
        }, arguments); },
        __wbg_set_binaryType_9a9ed83755a2cba4: function(arg0, arg1) {
            getObject(arg0).binaryType = __wbindgen_enum_RtcDataChannelType[arg1];
        },
        __wbg_set_body_1fb0f1008bfc7df6: function(arg0, arg1) {
            getObject(arg0).body = getObject(arg1);
        },
        __wbg_set_ice_restart_39a9f11be52c8df3: function(arg0, arg1) {
            getObject(arg0).iceRestart = arg1 !== 0;
        },
        __wbg_set_ice_servers_0e993347b54c4c75: function(arg0, arg1) {
            getObject(arg0).iceServers = getObject(arg1);
        },
        __wbg_set_method_dcb32343247ec427: function(arg0, arg1, arg2) {
            getObject(arg0).method = getStringFromWasm0(arg1, arg2);
        },
        __wbg_set_onclose_bb8c0565a5c25837: function(arg0, arg1) {
            getObject(arg0).onclose = getObject(arg1);
        },
        __wbg_set_onconnectionstatechange_9c3cc97053cc8cc7: function(arg0, arg1) {
            getObject(arg0).onconnectionstatechange = getObject(arg1);
        },
        __wbg_set_onicecandidate_d5e1ae20949a49ef: function(arg0, arg1) {
            getObject(arg0).onicecandidate = getObject(arg1);
        },
        __wbg_set_oniceconnectionstatechange_2b99e504c1f51e83: function(arg0, arg1) {
            getObject(arg0).oniceconnectionstatechange = getObject(arg1);
        },
        __wbg_set_onmessage_b1a84cbc1fe48890: function(arg0, arg1) {
            getObject(arg0).onmessage = getObject(arg1);
        },
        __wbg_set_onopen_6cc40bb77c7d158d: function(arg0, arg1) {
            getObject(arg0).onopen = getObject(arg1);
        },
        __wbg_set_ordered_c53f7b89fc9fb3b8: function(arg0, arg1) {
            getObject(arg0).ordered = arg1 !== 0;
        },
        __wbg_set_sdp_efdff02bfd21fa09: function(arg0, arg1, arg2) {
            getObject(arg0).sdp = getStringFromWasm0(arg1, arg2);
        },
        __wbg_set_type_6507077b45f3a9a5: function(arg0, arg1) {
            getObject(arg0).type = __wbindgen_enum_RtcSdpType[arg1];
        },
        __wbg_set_urls_str_3197af1e2601fb56: function(arg0, arg1, arg2) {
            getObject(arg0).urls = getStringFromWasm0(arg1, arg2);
        },
        __wbg_static_accessor_GLOBAL_266715b9d96ba635: function() {
            const ret = typeof global === 'undefined' ? null : global;
            return isLikeNone(ret) ? 0 : addHeapObject(ret);
        },
        __wbg_static_accessor_GLOBAL_THIS_10fb7dc1ae063179: function() {
            const ret = typeof globalThis === 'undefined' ? null : globalThis;
            return isLikeNone(ret) ? 0 : addHeapObject(ret);
        },
        __wbg_static_accessor_SELF_0b583911f537483a: function() {
            const ret = typeof self === 'undefined' ? null : self;
            return isLikeNone(ret) ? 0 : addHeapObject(ret);
        },
        __wbg_static_accessor_WINDOW_d7f903d1508cbdc4: function() {
            const ret = typeof window === 'undefined' ? null : window;
            return isLikeNone(ret) ? 0 : addHeapObject(ret);
        },
        __wbg_stringify_52ff602c1cc4fbb6: function() { return handleError(function (arg0) {
            const ret = JSON.stringify(getObject(arg0));
            return addHeapObject(ret);
        }, arguments); },
        __wbg_subarray_002b94d5e13d1411: function(arg0, arg1, arg2) {
            const ret = getObject(arg0).subarray(arg1 >>> 0, arg2 >>> 0);
            return addHeapObject(ret);
        },
        __wbg_text_ff3f476b3d6b1246: function() { return handleError(function (arg0) {
            const ret = getObject(arg0).text();
            return addHeapObject(ret);
        }, arguments); },
        __wbg_then_c949d5a25a4e78f8: function(arg0, arg1, arg2) {
            const ret = getObject(arg0).then(getObject(arg1), getObject(arg2));
            return addHeapObject(ret);
        },
        __wbg_then_e71170d78fcf8954: function(arg0, arg1) {
            const ret = getObject(arg0).then(getObject(arg1));
            return addHeapObject(ret);
        },
        __wbg_versions_276b2795b1c6a219: function(arg0) {
            const ret = getObject(arg0).versions;
            return addHeapObject(ret);
        },
        __wbindgen_generic_0000000000000001: function(arg0, arg1) {
            // Cast intrinsic for `Closure(Closure { owned: true, function: Function { arguments: [Externref], shim_idx: 838, ret: Result(Unit), inner_ret: Some(Result(Unit)) }, mutable: true }) -> Externref`.
            const ret = makeMutClosure(arg0, arg1, __wasm_bindgen_func_elem_5806);
            return addHeapObject(ret);
        },
        __wbindgen_generic_0000000000000002: function(arg0, arg1) {
            // Cast intrinsic for `Closure(Closure { owned: true, function: Function { arguments: [NamedExternref("MessageEvent")], shim_idx: 820, ret: Unit, inner_ret: Some(Unit) }, mutable: true }) -> Externref`.
            const ret = makeMutClosure(arg0, arg1, __wasm_bindgen_func_elem_4533);
            return addHeapObject(ret);
        },
        __wbindgen_generic_0000000000000003: function(arg0, arg1) {
            // Cast intrinsic for `Closure(Closure { owned: true, function: Function { arguments: [], shim_idx: 337, ret: Unit, inner_ret: Some(Unit) }, mutable: true }) -> Externref`.
            const ret = makeMutClosure(arg0, arg1, __wasm_bindgen_func_elem_2118);
            return addHeapObject(ret);
        },
        __wbindgen_generic_0000000000000004: function(arg0, arg1) {
            // Cast intrinsic for `Ref(Slice(U8)) -> NamedExternref("Uint8Array")`.
            const ret = getArrayU8FromWasm0(arg0, arg1);
            return addHeapObject(ret);
        },
        __wbindgen_generic_0000000000000005: function(arg0, arg1) {
            // Cast intrinsic for `Ref(String) -> Externref`.
            const ret = getStringFromWasm0(arg0, arg1);
            return addHeapObject(ret);
        },
        __wbindgen_object_clone_ref: function(arg0) {
            const ret = getObject(arg0);
            return addHeapObject(ret);
        },
        __wbindgen_object_drop_ref: function(arg0) {
            takeObject(arg0);
        },
    };
    return {
        __proto__: null,
        "./ephem_tor_bg.js": import0,
    };
}

function __wasm_bindgen_func_elem_2118(arg0, arg1) {
    wasm.__wasm_bindgen_func_elem_2118(arg0, arg1);
}

function __wasm_bindgen_func_elem_4533(arg0, arg1, arg2) {
    wasm.__wasm_bindgen_func_elem_4533(arg0, arg1, addHeapObject(arg2));
}

function __wasm_bindgen_func_elem_5841(arg0, arg1, arg2, arg3) {
    wasm.__wasm_bindgen_func_elem_5841(arg0, arg1, addHeapObject(arg2), addHeapObject(arg3));
}

function __wasm_bindgen_func_elem_5841_62(arg0, arg1, arg2, arg3) {
    wasm.__wasm_bindgen_func_elem_5841_62(arg0, arg1, addHeapObject(arg2), addHeapObject(arg3));
}

function __wasm_bindgen_func_elem_5806(arg0, arg1, arg2) {
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        wasm.__wasm_bindgen_func_elem_5806(retptr, arg0, arg1, addHeapObject(arg2));
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        if (r1) {
            throw takeObject(r0);
        }
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
    }
}


const __wbindgen_enum_RtcDataChannelState = ["connecting", "open", "closing", "closed"];


const __wbindgen_enum_RtcDataChannelType = ["arraybuffer", "blob"];


const __wbindgen_enum_RtcIceConnectionState = ["new", "checking", "connected", "completed", "failed", "disconnected", "closed"];


const __wbindgen_enum_RtcIceGatheringState = ["new", "gathering", "complete"];


const __wbindgen_enum_RtcPeerConnectionState = ["closed", "failed", "disconnected", "new", "connecting", "connected"];


const __wbindgen_enum_RtcSdpType = ["offer", "pranswer", "answer", "rollback"];
const AppFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_app_free(ptr, 1));

function addHeapObject(obj) {
    if (heap_next === heap.length) heap.push(heap.length + 1);
    const idx = heap_next;
    heap_next = heap[idx];

    heap[idx] = obj;
    return idx;
}

const CLOSURE_DTORS = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(state => wasm.__wbindgen_export4(state.a, state.b));

function debugString(val) {
    // primitive types
    const type = typeof val;
    if (type == 'number' || type == 'boolean' || val == null) {
        return  `${val}`;
    }
    if (type == 'string') {
        return `"${val}"`;
    }
    if (type == 'symbol') {
        const description = val.description;
        if (description == null) {
            return 'Symbol';
        } else {
            return `Symbol(${description})`;
        }
    }
    if (type == 'function') {
        const name = val.name;
        if (typeof name == 'string' && name.length > 0) {
            return `Function(${name})`;
        } else {
            return 'Function';
        }
    }
    // objects
    if (Array.isArray(val)) {
        const length = val.length;
        let debug = '[';
        if (length > 0) {
            debug += debugString(val[0]);
        }
        for(let i = 1; i < length; i++) {
            debug += ', ' + debugString(val[i]);
        }
        debug += ']';
        return debug;
    }
    // Test for built-in
    const builtInMatches = /\[object ([^\]]+)\]/.exec(toString.call(val));
    let className;
    if (builtInMatches && builtInMatches.length > 1) {
        className = builtInMatches[1];
    } else {
        // Failed to match the standard '[object ClassName]'
        return toString.call(val);
    }
    if (className == 'Object') {
        // we're a user defined class or Object
        // JSON.stringify avoids problems with cycles, and is generally much
        // easier than looping through ownProperties of `val`.
        try {
            return 'Object(' + JSON.stringify(val) + ')';
        } catch (_) {
            return 'Object';
        }
    }
    // errors
    if (val instanceof Error) {
        return `${val.name}: ${val.message}\n${val.stack}`;
    }
    // TODO we could test for more things here, like `Set`s and `Map`s.
    return className;
}

function dropObject(idx) {
    if (idx < 1028) return;
    heap[idx] = heap_next;
    heap_next = idx;
}

function getArrayU8FromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return getUint8ArrayMemory0().subarray(ptr / 1, ptr / 1 + len);
}

let cachedDataViewMemory0 = null;
function getDataViewMemory0() {
    if (cachedDataViewMemory0 === null || cachedDataViewMemory0.buffer.detached === true || (cachedDataViewMemory0.buffer.detached === undefined && cachedDataViewMemory0.buffer !== wasm.memory.buffer)) {
        cachedDataViewMemory0 = new DataView(wasm.memory.buffer);
    }
    return cachedDataViewMemory0;
}

function getStringFromWasm0(ptr, len) {
    return decodeText(ptr >>> 0, len);
}

let cachedUint8ArrayMemory0 = null;
function getUint8ArrayMemory0() {
    if (cachedUint8ArrayMemory0 === null || cachedUint8ArrayMemory0.byteLength === 0) {
        cachedUint8ArrayMemory0 = new Uint8Array(wasm.memory.buffer);
    }
    return cachedUint8ArrayMemory0;
}

function getObject(idx) { return heap[idx]; }

function handleError(f, args) {
    try {
        return f.apply(this, args);
    } catch (e) {
        wasm.__wbindgen_export3(addHeapObject(e));
    }
}

let heap = new Array(1024).fill(undefined);
heap.push(undefined, null, true, false);

let heap_next = heap.length;

function isLikeNone(x) {
    return x === undefined || x === null;
}

function makeMutClosure(arg0, arg1, f) {
    const state = { a: arg0, b: arg1, cnt: 1 };
    const real = (...args) => {

        // First up with a closure we increment the internal reference
        // count. This ensures that the Rust closure environment won't
        // be deallocated while we're invoking it.
        state.cnt++;
        const a = state.a;
        state.a = 0;
        try {
            return f(a, state.b, ...args);
        } finally {
            state.a = a;
            real._wbg_cb_unref();
        }
    };
    real._wbg_cb_unref = () => {
        if (--state.cnt === 0) {
            wasm.__wbindgen_export4(state.a, state.b);
            state.a = 0;
            CLOSURE_DTORS.unregister(state);
        }
    };
    CLOSURE_DTORS.register(real, state, state);
    return real;
}

function passArray8ToWasm0(arg, malloc) {
    const ptr = malloc(arg.length * 1, 1) >>> 0;
    getUint8ArrayMemory0().set(arg, ptr / 1);
    WASM_VECTOR_LEN = arg.length;
    return ptr;
}

function passStringToWasm0(arg, malloc, realloc) {
    if (realloc === undefined) {
        const buf = cachedTextEncoder.encode(arg);
        const ptr = malloc(buf.length, 1) >>> 0;
        getUint8ArrayMemory0().subarray(ptr, ptr + buf.length).set(buf);
        WASM_VECTOR_LEN = buf.length;
        return ptr;
    }

    let len = arg.length;
    let ptr = malloc(len, 1) >>> 0;

    const mem = getUint8ArrayMemory0();

    let offset = 0;

    for (; offset < len; offset++) {
        const code = arg.charCodeAt(offset);
        if (code > 0x7F) break;
        mem[ptr + offset] = code;
    }
    if (offset !== len) {
        if (offset !== 0) {
            arg = arg.slice(offset);
        }
        ptr = realloc(ptr, len, len = offset + arg.length * 3, 1) >>> 0;
        const view = getUint8ArrayMemory0().subarray(ptr + offset, ptr + len);
        const ret = cachedTextEncoder.encodeInto(arg, view);

        offset += ret.written;
        ptr = realloc(ptr, len, offset, 1) >>> 0;
    }

    WASM_VECTOR_LEN = offset;
    return ptr;
}

function takeObject(idx) {
    const ret = getObject(idx);
    dropObject(idx);
    return ret;
}

let cachedTextDecoder = new TextDecoder('utf-8', { ignoreBOM: true, fatal: true });
cachedTextDecoder.decode();
const MAX_SAFARI_DECODE_BYTES = 2146435072;
let numBytesDecoded = 0;
function decodeText(ptr, len) {
    numBytesDecoded += len;
    if (numBytesDecoded >= MAX_SAFARI_DECODE_BYTES) {
        cachedTextDecoder = new TextDecoder('utf-8', { ignoreBOM: true, fatal: true });
        cachedTextDecoder.decode();
        numBytesDecoded = len;
    }
    return cachedTextDecoder.decode(getUint8ArrayMemory0().subarray(ptr, ptr + len));
}

const cachedTextEncoder = new TextEncoder();

if (!('encodeInto' in cachedTextEncoder)) {
    cachedTextEncoder.encodeInto = function (arg, view) {
        const buf = cachedTextEncoder.encode(arg);
        view.set(buf);
        return {
            read: arg.length,
            written: buf.length
        };
    };
}

let WASM_VECTOR_LEN = 0;

let wasmModule, wasmInstance, wasm;
function __wbg_finalize_init(instance, module) {
    wasmInstance = instance;
    wasm = instance.exports;
    wasmModule = module;
    cachedDataViewMemory0 = null;
    cachedUint8ArrayMemory0 = null;
    return wasm;
}

async function __wbg_load(module, imports) {
    if (typeof Response === 'function' && module instanceof Response) {
        if (!module.ok) {
            throw new Error(`failed to fetch Wasm: ${module.status} ${module.statusText} fetching '${module.url}'`);
        }

        if (typeof WebAssembly.instantiateStreaming === 'function') {
            try {
                return await WebAssembly.instantiateStreaming(module, imports);
            } catch (e) {
                const validResponse = expectedResponseType(module.type);

                if (validResponse && module.headers.get('Content-Type') !== 'application/wasm') {
                    console.warn("`WebAssembly.instantiateStreaming` failed because your server does not serve Wasm with `application/wasm` MIME type. Falling back to `WebAssembly.instantiate` which is slower. Original error:\n", e);

                } else { throw e; }
            }
        }

        const bytes = await module.arrayBuffer();
        return await WebAssembly.instantiate(bytes, imports);
    } else {
        const instance = await WebAssembly.instantiate(module, imports);

        if (instance instanceof WebAssembly.Instance) {
            return { instance, module };
        } else {
            return instance;
        }
    }

    function expectedResponseType(type) {
        switch (type) {
            case 'basic': case 'cors': case 'default': return true;
        }
        return false;
    }
}

function initSync(module) {
    if (wasm !== undefined) return wasm;


    if (module !== undefined) {
        if (Object.getPrototypeOf(module) === Object.prototype) {
            ({module} = module)
        } else {
            console.warn('using deprecated parameters for `initSync()`; pass a single object instead')
        }
    }

    const imports = __wbg_get_imports();
    if (!(module instanceof WebAssembly.Module)) {
        module = new WebAssembly.Module(module);
    }
    const instance = new WebAssembly.Instance(module, imports);
    return __wbg_finalize_init(instance, module);
}

async function __wbg_init(module_or_path) {
    if (wasm !== undefined) return wasm;


    if (module_or_path !== undefined) {
        if (Object.getPrototypeOf(module_or_path) === Object.prototype) {
            ({module_or_path} = module_or_path)
        } else {
            console.warn('using deprecated parameters for the initialization function; pass a single object instead')
        }
    }

    if (module_or_path === undefined) {
        module_or_path = new URL('ephem_tor_bg.wasm', import.meta.url);
    }
    const imports = __wbg_get_imports();

    if (typeof module_or_path === 'string' || (typeof Request === 'function' && module_or_path instanceof Request) || (typeof URL === 'function' && module_or_path instanceof URL)) {
        module_or_path = fetch(module_or_path);
    }

    const { instance, module } = await __wbg_load(await module_or_path, imports);

    return __wbg_finalize_init(instance, module);
}

export { initSync, __wbg_init as default };
