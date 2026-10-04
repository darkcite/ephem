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
     * Adds the owner of a card as an unverified contact named `nick` (§7.5). Save the key file
     * afterwards.
     * @param {string} text
     * @param {string} nick
     * @returns {number}
     */
    add_card(text, nick) {
        const ptr0 = passStringToWasm0(text, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len0 = WASM_VECTOR_LEN;
        const ptr1 = passStringToWasm0(nick, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len1 = WASM_VECTOR_LEN;
        const ret = wasm.app_add_card(this.__wbg_ptr, ptr0, len0, ptr1, len1);
        return ret >>> 0;
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
     * Gives the page's channels (`ChannelApp`) this tab's Tor client and identity: channel keys
     * are derived from its seed (§D.3). Call again after every sign-in or sign-out.
     * @param {ChannelApp} ch
     */
    bind_channels(ch) {
        _assertClass(ch, ChannelApp);
        wasm.app_bind_channels(this.__wbg_ptr, ch.__wbg_ptr);
    }
    /**
     * Checks Tor bridge lines (Appendix F.2): JSON `{"usable", "bridges": [fingerprints],
     * "brokers", "stun", "problems": [{"line", "error", "text"}]}`.
     * @param {string} text
     * @returns {string}
     */
    bridges_check(text) {
        let deferred2_0;
        let deferred2_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            const ptr0 = passStringToWasm0(text, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            const len0 = WASM_VECTOR_LEN;
            wasm.app_bridges_check(retptr, this.__wbg_ptr, ptr0, len0);
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
     * Expiry of our current card (Unix seconds; 0 = never or no card).
     * @returns {number}
     */
    card_expires() {
        const ret = wasm.app_card_expires(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * The suggested nickname of a card (`#k=` link or text), or `None` if it is not a card.
     * @param {string} text
     * @returns {string | undefined}
     */
    card_nick(text) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            const ptr0 = passStringToWasm0(text, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            const len0 = WASM_VECTOR_LEN;
            wasm.app_card_nick(retptr, this.__wbg_ptr, ptr0, len0);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            let v2;
            if (r0 !== 0) {
                v2 = getStringFromWasm0(r0, r1);
                wasm.__wbindgen_export5(r0, r1 * 1, 1);
            }
            return v2;
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * How many chats the tab holds (at most [`MAX_CHATS`]).
     * @returns {number}
     */
    chat_count() {
        const ret = wasm.app_chat_count(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * The id of the selected chat: after a call that starts a conversation (invite, answer,
     * room, contact dial), the new chat's.
     * @returns {number}
     */
    chat() {
        const ret = wasm.app_chat(this.__wbg_ptr);
        return ret;
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
     * Leaves every chat (the tab closes).
     */
    close_all() {
        wasm.app_close_all(this.__wbg_ptr);
    }
    /**
     * Leaves the selected chat or room (a member first tells the owner; GOODBYE on every
     * link), wipes its session keys and messages. The chat is gone afterwards.
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
     * The contact fingerprint with contact `peer_hex` as `"1234 5678 9012"` (docs/CONTACTS-UX.md
     * §3.2); empty if it is not a contact.
     * @param {string} peer_hex
     * @returns {string}
     */
    contact_fingerprint(peer_hex) {
        let deferred2_0;
        let deferred2_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            const ptr0 = passStringToWasm0(peer_hex, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            const len0 = WASM_VECTOR_LEN;
            wasm.app_contact_fingerprint(retptr, this.__wbg_ptr, ptr0, len0);
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
     * Creates a room owned by us, as a new chat. Invite members next.
     * @returns {number}
     */
    create_room() {
        const ret = wasm.app_create_room(this.__wbg_ptr);
        return ret >>> 0;
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
     * Our contact card as base64url (empty for a temporary identity). A card is created on
     * first use, and again when `reset` or when the current one expired; `ttl_days` = 0 makes
     * a new card that never expires. Save the key file afterwards (the secret is in it).
     * @param {boolean} reset
     * @param {number} ttl_days
     * @returns {string}
     */
    my_card(reset, ttl_days) {
        let deferred1_0;
        let deferred1_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.app_my_card(retptr, this.__wbg_ptr, reset, ttl_days);
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
     * Section `t` of the key file (0x05 Tor bridge lines, 0x06 followed channels) as UTF-8, or
     * empty.
     * @param {number} t
     * @returns {string}
     */
    section(t) {
        let deferred1_0;
        let deferred1_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.app_section(retptr, this.__wbg_ptr, t);
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
     * Selects chat `id` for the calls that follow (call it right before them, in the same
     * task: network events may load another chat in between). False if there is no such chat.
     * @param {number} id
     * @returns {boolean}
     */
    select(id) {
        const ret = wasm.app_select(this.__wbg_ptr, id);
        return ret !== 0;
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
     * Replaces section `t` (empty text removes it). Save the key file afterwards.
     * @param {number} t
     * @param {string} text
     * @returns {number}
     */
    set_section(t, text) {
        const ptr0 = passStringToWasm0(text, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.app_set_section(this.__wbg_ptr, t, ptr0, len0);
        return ret >>> 0;
    }
    /**
     * The user's own STUN servers (§9.3, where the defaults are blocked): up to
     * [`rtc::MAX_STUN`] `stun:host[:port]` URLs, separated by commas, spaces or new lines;
     * empty = the default list. Used by links started from now on. Returns how many are used, or
     * 0 if one is not a valid `stun:` URL (then nothing changes: TURN is never accepted, §9.2).
     * @param {string} list
     * @returns {number}
     */
    set_stun(list) {
        const ptr0 = passStringToWasm0(list, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.app_set_stun(this.__wbg_ptr, ptr0, len0);
        return ret >>> 0;
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
     * The Tor directory as a gzip snapshot for IndexedDB (public data; empty until
     * downloaded). Copied out to JS once per save (every 30 min).
     * @returns {Uint8Array}
     */
    tor_cache() {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.app_tor_cache(retptr, this.__wbg_ptr);
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
     * arti logs to the console at `level` (`"info"`, `"debug"`, …; diagnostics only).
     * @param {string} level
     */
    tor_log(level) {
        const ptr0 = passStringToWasm0(level, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len0 = WASM_VECTOR_LEN;
        wasm.app_tor_log(this.__wbg_ptr, ptr0, len0);
    }
    /**
     * Whether peers can reach our onion yet: `publishing`, `reachable`, `degraded`,
     * `unreachable`, `down`, or empty before it is hosted.
     * @returns {string}
     */
    tor_reach() {
        let deferred1_0;
        let deferred1_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.app_tor_reach(retptr, this.__wbg_ptr);
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
     * Redials at once every chat whose Tor stream was lost and that we dialled (the page came
     * back to the foreground, the network returned, or "Reconnect now"): fresh circuits, no
     * waiting out the backoff. The host side has nothing to do: its peer dials it.
     */
    tor_redial_now() {
        wasm.app_tor_redial_now(this.__wbg_ptr);
    }
    /**
     * Starts arti over Snowflake, then hosts our onion service (key from the identity seed).
     * Progress arrives as TOR events. `bridges`: Snowflake bridge lines in the Tor Browser
     * format (the defaults or the user's, Appendix F.2; see `bridges_check`); `nat`: the
     * broker's NAT hint (empty = "unknown"); `network_toml`: empty for the real Tor network;
     * `cache`: the directory snapshot of `tor_cache` from an earlier session, or empty.
     * @param {string} bridges
     * @param {string} nat
     * @param {string} network_toml
     * @param {Uint8Array} cache
     * @returns {number}
     */
    tor_start(bridges, nat, network_toml, cache) {
        const ptr0 = passStringToWasm0(bridges, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len0 = WASM_VECTOR_LEN;
        const ptr1 = passStringToWasm0(nat, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len1 = WASM_VECTOR_LEN;
        const ptr2 = passStringToWasm0(network_toml, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len2 = WASM_VECTOR_LEN;
        const ptr3 = passArray8ToWasm0(cache, wasm.__wbindgen_export);
        const len3 = WASM_VECTOR_LEN;
        const ret = wasm.app_tor_start(this.__wbg_ptr, ptr0, len0, ptr1, len1, ptr2, len2, ptr3, len3);
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
    /**
     * Marks contact `peer_hex` verified (its fingerprint was compared in person). Re-save after.
     * @param {string} peer_hex
     * @returns {number}
     */
    verify_contact(peer_hex) {
        const ptr0 = passStringToWasm0(peer_hex, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.app_verify_contact(this.__wbg_ptr, ptr0, len0);
        return ret >>> 0;
    }
}
if (Symbol.dispose) App.prototype[Symbol.dispose] = App.prototype.free;

export class BoardApp {
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        BoardAppFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_boardapp_free(ptr, 0);
    }
    /**
     * Approves held post `i`: it is numbered and published. Returns its number.
     * @param {number} index
     * @param {number} i
     * @returns {number}
     */
    approve(index, i) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.boardapp_approve(retptr, this.__wbg_ptr, index, i);
            var r0 = getDataViewMemory0().getFloat64(retptr + 8 * 0, true);
            var r2 = getDataViewMemory0().getInt32(retptr + 4 * 2, true);
            var r3 = getDataViewMemory0().getInt32(retptr + 4 * 3, true);
            if (r3) {
                throw takeObject(r2);
            }
            return r0;
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * Approves (or withdraws) the trip of post `no` for the approved-trips switch.
     * @param {number} index
     * @param {number} no
     * @param {boolean} on
     */
    approve_trip_of(index, no, on) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.boardapp_approve_trip_of(retptr, this.__wbg_ptr, index, no, on);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            if (r1) {
                throw takeObject(r0);
            }
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * Bans the key of post `no`; returns the key (64 hex digits) for `unban`.
     * @param {number} index
     * @param {number} no
     * @param {string} why
     * @returns {string}
     */
    ban(index, no, why) {
        let deferred3_0;
        let deferred3_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            const ptr0 = passStringToWasm0(why, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            const len0 = WASM_VECTOR_LEN;
            wasm.boardapp_ban(retptr, this.__wbg_ptr, index, no, ptr0, len0);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var r2 = getDataViewMemory0().getInt32(retptr + 4 * 2, true);
            var r3 = getDataViewMemory0().getInt32(retptr + 4 * 3, true);
            var ptr2 = r0;
            var len2 = r1;
            if (r3) {
                ptr2 = 0; len2 = 0;
                throw takeObject(r2);
            }
            deferred3_0 = ptr2;
            deferred3_1 = len2;
            return getStringFromWasm0(ptr2, len2);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
            wasm.__wbindgen_export5(deferred3_0, deferred3_1, 1);
        }
    }
    /**
     * Board `index`'s own onion address (the same on every device of the identity).
     * @param {number} index
     * @returns {string}
     */
    board_onion(index) {
        let deferred2_0;
        let deferred2_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.boardapp_board_onion(retptr, this.__wbg_ptr, index);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var r2 = getDataViewMemory0().getInt32(retptr + 4 * 2, true);
            var r3 = getDataViewMemory0().getInt32(retptr + 4 * 3, true);
            var ptr1 = r0;
            var len1 = r1;
            if (r3) {
                ptr1 = 0; len1 = 0;
                throw takeObject(r2);
            }
            deferred2_0 = ptr1;
            deferred2_1 = len1;
            return getStringFromWasm0(ptr1, len1);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
            wasm.__wbindgen_export5(deferred2_0, deferred2_1, 1);
        }
    }
    /**
     * Closes board `index` here (its onion goes down; the store keeps it).
     * @param {number} index
     */
    close(index) {
        wasm.boardapp_close(this.__wbg_ptr, index);
    }
    /**
     * Continues board `index` without its blocks (G.13.9, no source answered): an empty
     * catalog under the same name and onion, numbers from `next_no` on, records above `seq`.
     * @param {number} index
     * @param {string} title
     * @param {number} next_no
     * @param {number} seq
     * @param {string} mirrors
     */
    continue_board(index, title, next_no, seq, mirrors) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            const ptr0 = passStringToWasm0(title, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            const len0 = WASM_VECTOR_LEN;
            const ptr1 = passStringToWasm0(mirrors, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            const len1 = WASM_VECTOR_LEN;
            wasm.boardapp_continue_board(retptr, this.__wbg_ptr, index, ptr0, len0, next_no, seq, ptr1, len1);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            if (r1) {
                throw takeObject(r0);
            }
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * A new board `index` (the page refuses when its store already holds one).
     * @param {number} index
     * @param {string} title
     * @param {string} about
     * @param {string} rules
     */
    create(index, title, about, rules) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            const ptr0 = passStringToWasm0(title, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            const len0 = WASM_VECTOR_LEN;
            const ptr1 = passStringToWasm0(about, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            const len1 = WASM_VECTOR_LEN;
            const ptr2 = passStringToWasm0(rules, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            const len2 = WASM_VECTOR_LEN;
            wasm.boardapp_create(retptr, this.__wbg_ptr, index, ptr0, len0, ptr1, len1, ptr2, len2);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            if (r1) {
                throw takeObject(r0);
            }
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * The owner deletes post `no` (an OP takes its thread), published after the 5 s undo
     * window (`undo`).
     * @param {number} index
     * @param {number} no
     */
    delete(index, no) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.boardapp_delete(retptr, this.__wbg_ptr, index, no);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            if (r1) {
                throw takeObject(r0);
            }
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * Mass delete: every post signed with the key of post `no` (a trip, or an IDs-on key).
     * @param {number} index
     * @param {number} no
     * @returns {number}
     */
    delete_by_key_of(index, no) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.boardapp_delete_by_key_of(retptr, this.__wbg_ptr, index, no);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var r2 = getDataViewMemory0().getInt32(retptr + 4 * 2, true);
            if (r2) {
                throw takeObject(r1);
            }
            return r0 >>> 0;
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * Mass delete: every post from No. `no` on.
     * @param {number} index
     * @param {number} no
     * @returns {number}
     */
    delete_from(index, no) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.boardapp_delete_from(retptr, this.__wbg_ptr, index, no);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var r2 = getDataViewMemory0().getInt32(retptr + 4 * 2, true);
            if (r2) {
                throw takeObject(r1);
            }
            return r0 >>> 0;
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * Mass delete: every post since `since_s` (Unix seconds). Returns how many.
     * @param {number} index
     * @param {number} since_s
     * @returns {number}
     */
    delete_since(index, since_s) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.boardapp_delete_since(retptr, this.__wbg_ptr, index, since_s);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var r2 = getDataViewMemory0().getInt32(retptr + 4 * 2, true);
            if (r2) {
                throw takeObject(r1);
            }
            return r0 >>> 0;
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * What the store must write and delete since the last call: `{record, added: [[cid,
     * bytes]…], removed: [cid…]}` (each block copied into JS once).
     * @param {number} index
     * @returns {object}
     */
    delta(index) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.boardapp_delta(retptr, this.__wbg_ptr, index);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var r2 = getDataViewMemory0().getInt32(retptr + 4 * 2, true);
            if (r2) {
                throw takeObject(r1);
            }
            return takeObject(r0);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * Opens a reply box (G.6.1 step 1): `GET /pow` from the board's onion, and the poster key:
     * a fresh one per post, or with `trip` (a label, signed in) the identity's trip key for
     * this board and label (G.4). `thread` 0 = a new thread.
     * @param {string} name
     * @param {string} onion
     * @param {number} thread
     * @param {string} trip
     * @returns {Promise<any>}
     */
    draft(name, onion, thread, trip) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            const ptr0 = passStringToWasm0(name, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            const len0 = WASM_VECTOR_LEN;
            const ptr1 = passStringToWasm0(onion, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            const len1 = WASM_VECTOR_LEN;
            const ptr2 = passStringToWasm0(trip, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            const len2 = WASM_VECTOR_LEN;
            wasm.boardapp_draft(retptr, this.__wbg_ptr, ptr0, len0, ptr1, len1, thread, ptr2, len2);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var r2 = getDataViewMemory0().getInt32(retptr + 4 * 2, true);
            if (r2) {
                throw takeObject(r1);
            }
            return takeObject(r0);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * The posts held for approval: `[{i, at, t, sub, body, trip}]`.
     * @param {number} index
     * @returns {string}
     */
    held(index) {
        let deferred2_0;
        let deferred2_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.boardapp_held(retptr, this.__wbg_ptr, index);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var r2 = getDataViewMemory0().getInt32(retptr + 4 * 2, true);
            var r3 = getDataViewMemory0().getInt32(retptr + 4 * 3, true);
            var ptr1 = r0;
            var len1 = r1;
            if (r3) {
                ptr1 = 0; len1 = 0;
                throw takeObject(r2);
            }
            deferred2_0 = ptr1;
            deferred2_1 = len1;
            return getStringFromWasm0(ptr1, len1);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
            wasm.__wbindgen_export5(deferred2_0, deferred2_1, 1);
        }
    }
    /**
     * Mirrors board `name` on this tab's onion for it (`seed`, 32 bytes, kept by the page, so
     * the address is stable and the owner can sign it in). Pulls from `onions` (the owner's,
     * then other mirrors) now and every [`PULL_MS`], fetching only threads that changed and
     * verifying them; serves read-only (`/pow` and `/submit` answer `E_BOARD_OFFLINE`).
     * Resolves to `<56 chars>.onion`.
     * @param {string} name
     * @param {string} onions
     * @param {Uint8Array} seed
     * @returns {Promise<any>}
     */
    mirror(name, onions, seed) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            const ptr0 = passStringToWasm0(name, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            const len0 = WASM_VECTOR_LEN;
            const ptr1 = passStringToWasm0(onions, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            const len1 = WASM_VECTOR_LEN;
            const ptr2 = passArray8ToWasm0(seed, wasm.__wbindgen_export);
            const len2 = WASM_VECTOR_LEN;
            wasm.boardapp_mirror(retptr, this.__wbg_ptr, ptr0, len0, ptr1, len1, ptr2, len2);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var r2 = getDataViewMemory0().getInt32(retptr + 4 * 2, true);
            if (r2) {
                throw takeObject(r1);
            }
            return takeObject(r0);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * What mirror `name`'s store must write and delete since the last call (as `delta`).
     * @param {string} name
     * @returns {object}
     */
    mirror_delta(name) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            const ptr0 = passStringToWasm0(name, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            const len0 = WASM_VECTOR_LEN;
            wasm.boardapp_mirror_delta(retptr, this.__wbg_ptr, ptr0, len0);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var r2 = getDataViewMemory0().getInt32(retptr + 4 * 2, true);
            if (r2) {
                throw takeObject(r1);
            }
            return takeObject(r0);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * Serves board `name`'s mirror at once from this tab's store (`record`, `[[cid, bytes]…]`),
     * owner online or not, then keeps it current from `onions` as `mirror` does. The copy must
     * verify (an expired record is served, readers mark it stale) and be complete. Returns
     * the mirror's onion.
     * @param {string} name
     * @param {string} onions
     * @param {Uint8Array} seed
     * @param {Uint8Array} record
     * @param {Array<any>} blocks
     * @returns {string}
     */
    mirror_open(name, onions, seed, record, blocks) {
        let deferred6_0;
        let deferred6_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            const ptr0 = passStringToWasm0(name, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            const len0 = WASM_VECTOR_LEN;
            const ptr1 = passStringToWasm0(onions, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            const len1 = WASM_VECTOR_LEN;
            const ptr2 = passArray8ToWasm0(seed, wasm.__wbindgen_export);
            const len2 = WASM_VECTOR_LEN;
            const ptr3 = passArray8ToWasm0(record, wasm.__wbindgen_export);
            const len3 = WASM_VECTOR_LEN;
            wasm.boardapp_mirror_open(retptr, this.__wbg_ptr, ptr0, len0, ptr1, len1, ptr2, len2, ptr3, len3, addHeapObject(blocks));
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var r2 = getDataViewMemory0().getInt32(retptr + 4 * 2, true);
            var r3 = getDataViewMemory0().getInt32(retptr + 4 * 3, true);
            var ptr5 = r0;
            var len5 = r1;
            if (r3) {
                ptr5 = 0; len5 = 0;
                throw takeObject(r2);
            }
            deferred6_0 = ptr5;
            deferred6_1 = len5;
            return getStringFromWasm0(ptr5, len5);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
            wasm.__wbindgen_export5(deferred6_0, deferred6_1, 1);
        }
    }
    /**
     * The onion seed of this identity's mirror of board `name` (stable across visits, BF-5);
     * the page passes it to `mirror`. Needs a signed-in identity.
     * @param {string} name
     * @returns {Uint8Array}
     */
    mirror_seed(name) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            const ptr0 = passStringToWasm0(name, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            const len0 = WASM_VECTOR_LEN;
            wasm.boardapp_mirror_seed(retptr, this.__wbg_ptr, ptr0, len0);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var r2 = getDataViewMemory0().getInt32(retptr + 4 * 2, true);
            var r3 = getDataViewMemory0().getInt32(retptr + 4 * 3, true);
            if (r3) {
                throw takeObject(r2);
            }
            var v2 = getArrayU8FromWasm0(r0, r1).slice();
            wasm.__wbindgen_export5(r0, r1 * 1, 1);
            return v2;
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * The sequence a mirrored board is at (0: not mirrored here).
     * @param {string} name
     * @returns {number}
     */
    mirror_seq(name) {
        const ptr0 = passStringToWasm0(name, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.boardapp_mirror_seq(this.__wbg_ptr, ptr0, len0);
        return ret;
    }
    /**
     * Board `index`'s IPNS name (`k51…`), the same on every device of the identity.
     * @param {number} index
     * @returns {string}
     */
    name(index) {
        let deferred2_0;
        let deferred2_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.boardapp_name(retptr, this.__wbg_ptr, index);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var r2 = getDataViewMemory0().getInt32(retptr + 4 * 2, true);
            var r3 = getDataViewMemory0().getInt32(retptr + 4 * 3, true);
            var ptr1 = r0;
            var len1 = r1;
            if (r3) {
                ptr1 = 0; len1 = 0;
                throw takeObject(r2);
            }
            deferred2_0 = ptr1;
            deferred2_1 = len1;
            return getStringFromWasm0(ptr1, len1);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
            wasm.__wbindgen_export5(deferred2_0, deferred2_1, 1);
        }
    }
    /**
     * Boards of the page's channels engine (same identity and Tor client).
     * @param {ChannelApp} ch
     */
    constructor(ch) {
        _assertClass(ch, ChannelApp);
        const ret = wasm.boardapp_new(ch.__wbg_ptr);
        this.__wbg_ptr = ret;
        BoardAppFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
    /**
     * Reopens board `index` from its store: the last record and its blocks (`[[cid, bytes]…]`).
     * @param {number} index
     * @param {Uint8Array} record
     * @param {Array<any>} blocks
     */
    open(index, record, blocks) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            const ptr0 = passArray8ToWasm0(record, wasm.__wbindgen_export);
            const len0 = WASM_VECTOR_LEN;
            wasm.boardapp_open(retptr, this.__wbg_ptr, index, ptr0, len0, addHeapObject(blocks));
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            if (r1) {
                throw takeObject(r0);
            }
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * Indices of the boards open here.
     * @returns {Uint32Array}
     */
    open_boards() {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.boardapp_open_boards(retptr, this.__wbg_ptr);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var v1 = getArrayU32FromWasm0(r0, r1).slice();
            wasm.__wbindgen_export5(r0, r1 * 4, 4);
            return v1;
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * The owner's own board as readers see it (JSON as `read`), from what this tab serves: the
     * index and the `threads` asked for, verified locally (no Tor round trip).
     * @param {number} index
     * @param {Float64Array} threads
     * @returns {string}
     */
    owner_view(index, threads) {
        let deferred3_0;
        let deferred3_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            const ptr0 = passArrayF64ToWasm0(threads, wasm.__wbindgen_export);
            const len0 = WASM_VECTOR_LEN;
            wasm.boardapp_owner_view(retptr, this.__wbg_ptr, index, ptr0, len0);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var r2 = getDataViewMemory0().getInt32(retptr + 4 * 2, true);
            var r3 = getDataViewMemory0().getInt32(retptr + 4 * 3, true);
            var ptr2 = r0;
            var len2 = r1;
            if (r3) {
                ptr2 = 0; len2 = 0;
                throw takeObject(r2);
            }
            deferred3_0 = ptr2;
            deferred3_1 = len2;
            return getStringFromWasm0(ptr2, len2);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
            wasm.__wbindgen_export5(deferred3_0, deferred3_1, 1);
        }
    }
    /**
     * The owner posts (capcode, no proof of work): a new thread (`thread` 0) or a reply.
     * @param {number} index
     * @param {number} thread
     * @param {string} sub
     * @param {string} body
     * @param {boolean} sage
     * @returns {number}
     */
    post(index, thread, sub, body, sage) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            const ptr0 = passStringToWasm0(sub, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            const len0 = WASM_VECTOR_LEN;
            const ptr1 = passStringToWasm0(body, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            const len1 = WASM_VECTOR_LEN;
            wasm.boardapp_post(retptr, this.__wbg_ptr, index, thread, ptr0, len0, ptr1, len1, sage);
            var r0 = getDataViewMemory0().getFloat64(retptr + 8 * 0, true);
            var r2 = getDataViewMemory0().getInt32(retptr + 4 * 2, true);
            var r3 = getDataViewMemory0().getInt32(retptr + 4 * 3, true);
            if (r3) {
                throw takeObject(r2);
            }
            return r0;
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * Signs and submits the post solved for `draft` (`n`, `solution` from a Worker), on a new
     * Tor isolation group. Resolves to `{no, seq}` as JSON; a refusal rejects with its reason.
     * @param {Draft} draft
     * @param {string} sub
     * @param {string} body
     * @param {boolean} sage
     * @param {Uint8Array} n
     * @param {Uint8Array} solution
     * @returns {Promise<any>}
     */
    post_draft(draft, sub, body, sage, n, solution) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            _assertClass(draft, Draft);
            const ptr0 = passStringToWasm0(sub, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            const len0 = WASM_VECTOR_LEN;
            const ptr1 = passStringToWasm0(body, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            const len1 = WASM_VECTOR_LEN;
            const ptr2 = passArray8ToWasm0(n, wasm.__wbindgen_export);
            const len2 = WASM_VECTOR_LEN;
            const ptr3 = passArray8ToWasm0(solution, wasm.__wbindgen_export);
            const len3 = WASM_VECTOR_LEN;
            wasm.boardapp_post_draft(retptr, this.__wbg_ptr, draft.__wbg_ptr, ptr0, len0, ptr1, len1, sage, ptr2, len2, ptr3, len3);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var r2 = getDataViewMemory0().getInt32(retptr + 4 * 2, true);
            if (r2) {
                throw takeObject(r1);
            }
            return takeObject(r0);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * @param {number} index
     * @param {number} no
     */
    prune(index, no) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.boardapp_prune(retptr, this.__wbg_ptr, index, no);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            if (r1) {
                throw takeObject(r0);
            }
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * Publishes board `name`'s current record to the IPFS routing network (G.5.3, optional):
     * `PUT https://<host>/routing/v1/ipns/<name>` through a Tor exit, as channels do. The owner's
     * own record, or a mirror's copy while it is unexpired (mirrors cannot extend validity). The
     * page calls it at most every 10 minutes.
     * @param {string} name
     * @param {string} host
     * @param {Uint8Array} extra_root
     * @returns {Promise<any>}
     */
    publish_ipfs(name, host, extra_root) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            const ptr0 = passStringToWasm0(name, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            const len0 = WASM_VECTOR_LEN;
            const ptr1 = passStringToWasm0(host, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            const len1 = WASM_VECTOR_LEN;
            const ptr2 = passArray8ToWasm0(extra_root, wasm.__wbindgen_export);
            const len2 = WASM_VECTOR_LEN;
            wasm.boardapp_publish_ipfs(retptr, this.__wbg_ptr, ptr0, len0, ptr1, len1, ptr2, len2);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var r2 = getDataViewMemory0().getInt32(retptr + 4 * 2, true);
            if (r2) {
                throw takeObject(r1);
            }
            return takeObject(r0);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * Whether readers can reach board `index`'s onion yet (as channels' `reach`).
     * @param {number} index
     * @returns {string}
     */
    reach(index) {
        let deferred1_0;
        let deferred1_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.boardapp_reach(retptr, this.__wbg_ptr, index);
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
     * Reads board `name` from the first onion (comma-separated: owner, mirrors) that serves a
     * valid state, with the `threads` asked for (numbers). Resolves to the JSON view
     * ([`view_json`]).
     * `fresh`: every request on new circuits from the first round, so the onion's descriptor is
     * fetched again (a reader whose Tor client kept the descriptor of a device that no longer
     * hosts the board, G.13: "Try again on a fresh connection").
     * @param {string} name
     * @param {string} onions
     * @param {number} min_seq
     * @param {Float64Array} threads
     * @param {boolean} fresh
     * @returns {Promise<any>}
     */
    read(name, onions, min_seq, threads, fresh) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            const ptr0 = passStringToWasm0(name, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            const len0 = WASM_VECTOR_LEN;
            const ptr1 = passStringToWasm0(onions, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            const len1 = WASM_VECTOR_LEN;
            const ptr2 = passArrayF64ToWasm0(threads, wasm.__wbindgen_export);
            const len2 = WASM_VECTOR_LEN;
            wasm.boardapp_read(retptr, this.__wbg_ptr, ptr0, len0, ptr1, len1, min_seq, ptr2, len2, fresh);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var r2 = getDataViewMemory0().getInt32(retptr + 4 * 2, true);
            if (r2) {
                throw takeObject(r1);
            }
            return takeObject(r0);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * @param {number} index
     * @param {number} i
     */
    reject(index, i) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.boardapp_reject(retptr, this.__wbg_ptr, index, i);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            if (r1) {
                throw takeObject(r0);
            }
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * Sends the same submit again (a dropped stream): the host answers with the original
     * `{no, seq}` if it was published.
     * @param {Draft} draft
     * @returns {Promise<any>}
     */
    resend(draft) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            _assertClass(draft, Draft);
            wasm.boardapp_resend(retptr, this.__wbg_ptr, draft.__wbg_ptr);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var r2 = getDataViewMemory0().getInt32(retptr + 4 * 2, true);
            if (r2) {
                throw takeObject(r1);
            }
            return takeObject(r0);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * Serves board `index` on its own onion (G.4: unlinked from the chat onion and channels);
     * returns `<56 chars>.onion`.
     * @param {number} index
     * @returns {string}
     */
    serve(index) {
        let deferred2_0;
        let deferred2_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.boardapp_serve(retptr, this.__wbg_ptr, index);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var r2 = getDataViewMemory0().getInt32(retptr + 4 * 2, true);
            var r3 = getDataViewMemory0().getInt32(retptr + 4 * 3, true);
            var ptr1 = r0;
            var len1 = r1;
            if (r3) {
                ptr1 = 0; len1 = 0;
                throw takeObject(r2);
            }
            deferred2_0 = ptr1;
            deferred2_1 = len1;
            return getStringFromWasm0(ptr1, len1);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
            wasm.__wbindgen_export5(deferred2_0, deferred2_1, 1);
        }
    }
    /**
     * The owner's base efforts (G.8; the adaptive multiplier applies on top). At least 1.
     * @param {number} index
     * @param {number} reply
     * @param {number} thread
     */
    set_efforts(index, reply, thread) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.boardapp_set_efforts(retptr, this.__wbg_ptr, index, reply, thread);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            if (r1) {
                throw takeObject(r0);
            }
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * Called with the board's index after each publish (the page then takes the `delta`).
     * @param {Function} f
     */
    set_listener(f) {
        wasm.boardapp_set_listener(this.__wbg_ptr, addHeapObject(f));
    }
    /**
     * @param {number} index
     * @param {number} no
     * @param {boolean} on
     */
    set_locked(index, no, on) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.boardapp_set_locked(retptr, this.__wbg_ptr, index, no, on);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            if (r1) {
                throw takeObject(r0);
            }
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * Signs the mirror list (comma-separated onion addresses, ≤ 8) into board `index`'s
     * manifest (G.10): readers try them when the owner's onion does not answer.
     * @param {number} index
     * @param {string} csv
     */
    set_mirrors(index, csv) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            const ptr0 = passStringToWasm0(csv, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            const len0 = WASM_VECTOR_LEN;
            wasm.boardapp_set_mirrors(retptr, this.__wbg_ptr, index, ptr0, len0);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            if (r1) {
                throw takeObject(r0);
            }
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * Signs "see also" links into board `index`'s manifest (comma-separated `<name>@<onion>`).
     * @param {number} index
     * @param {string} csv
     */
    set_see_also(index, csv) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            const ptr0 = passStringToWasm0(csv, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            const len0 = WASM_VECTOR_LEN;
            wasm.boardapp_set_see_also(retptr, this.__wbg_ptr, index, ptr0, len0);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            if (r1) {
                throw takeObject(r0);
            }
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * @param {number} index
     * @param {number} no
     * @param {boolean} on
     */
    set_sticky(index, no, on) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.boardapp_set_sticky(retptr, this.__wbg_ptr, index, no, on);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            if (r1) {
                throw takeObject(r0);
            }
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * @param {number} index
     * @param {boolean} paused
     * @param {boolean} threads_closed
     * @param {boolean} trips_only
     * @param {boolean} approved_only
     * @param {boolean} premod
     * @param {boolean} panic_trips
     */
    set_switches(index, paused, threads_closed, trips_only, approved_only, premod, panic_trips) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.boardapp_set_switches(retptr, this.__wbg_ptr, index, paused, threads_closed, trips_only, approved_only, premod, panic_trips);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            if (r1) {
                throw takeObject(r0);
            }
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * The owner's view of board `index`: `{name, onion, seq, threads, next_no, paused,
     * threads_closed, closed_notice, effort_reply, effort_thread, blocks, held, pending_deletes, bans, base_reply, base_thread}`.
     * @param {number} index
     * @returns {string}
     */
    status(index) {
        let deferred1_0;
        let deferred1_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.boardapp_status(retptr, this.__wbg_ptr, index);
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
     * The switches as JSON `{paused, threads_closed, trips_only, approved_only, premod,
     * panic_trips}`.
     * @param {number} index
     * @returns {string}
     */
    switches(index) {
        let deferred2_0;
        let deferred2_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.boardapp_switches(retptr, this.__wbg_ptr, index);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var r2 = getDataViewMemory0().getInt32(retptr + 4 * 2, true);
            var r3 = getDataViewMemory0().getInt32(retptr + 4 * 3, true);
            var ptr1 = r0;
            var len1 = r1;
            if (r3) {
                ptr1 = 0; len1 = 0;
                throw takeObject(r2);
            }
            deferred2_0 = ptr1;
            deferred2_1 = len1;
            return getStringFromWasm0(ptr1, len1);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
            wasm.__wbindgen_export5(deferred2_0, deferred2_1, 1);
        }
    }
    /**
     * "Host this board here": board `index` is read in full (index, every thread, the
     * archive and the encrypted `own` block) from `onions` (its own address, served by the
     * other device, then its mirrors), all at once, verified, and the newest complete version
     * is hosted here; nothing older than the vault's `floor_seq` (BW-2), `root` preferred on a
     * tie. Numbers continue from at least `next_no_floor` (G.13.6, computed by the page from
     * the vault).
     * @param {number} index
     * @param {string} onions
     * @param {number} next_no_floor
     * @param {number} floor_seq
     * @param {string} root
     * @returns {Promise<any>}
     */
    take_over(index, onions, next_no_floor, floor_seq, root) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            const ptr0 = passStringToWasm0(onions, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            const len0 = WASM_VECTOR_LEN;
            const ptr1 = passStringToWasm0(root, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            const len1 = WASM_VECTOR_LEN;
            wasm.boardapp_take_over(retptr, this.__wbg_ptr, index, ptr0, len0, next_no_floor, floor_seq, ptr1, len1);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var r2 = getDataViewMemory0().getInt32(retptr + 4 * 2, true);
            if (r2) {
                throw takeObject(r1);
            }
            return takeObject(r0);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * @param {number} index
     * @param {string} key_hex
     */
    unban(index, key_hex) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            const ptr0 = passStringToWasm0(key_hex, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            const len0 = WASM_VECTOR_LEN;
            wasm.boardapp_unban(retptr, this.__wbg_ptr, index, ptr0, len0);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            if (r1) {
                throw takeObject(r0);
            }
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * Undoes a pending delete (`no` 0: all of them). Returns how many.
     * @param {number} index
     * @param {number} no
     * @returns {number}
     */
    undo(index, no) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.boardapp_undo(retptr, this.__wbg_ptr, index, no);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var r2 = getDataViewMemory0().getInt32(retptr + 4 * 2, true);
            if (r2) {
                throw takeObject(r1);
            }
            return r0 >>> 0;
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * Stops mirroring board `name` (its mirror onion goes down).
     * @param {string} name
     */
    unmirror(name) {
        const ptr0 = passStringToWasm0(name, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len0 = WASM_VECTOR_LEN;
        wasm.boardapp_unmirror(this.__wbg_ptr, ptr0, len0);
    }
    /**
     * Publishes the vault with this device's lease on the boards it hosts (G.13.2); the
     * channels' lease and entries stay as last read. As `ChannelApp.vault_publish`.
     * @param {string} host
     * @param {Uint8Array} extra_root
     * @param {string} device
     * @param {number} until
     * @returns {Promise<any>}
     */
    vault_publish(host, extra_root, device, until) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            const ptr0 = passStringToWasm0(host, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            const len0 = WASM_VECTOR_LEN;
            const ptr1 = passArray8ToWasm0(extra_root, wasm.__wbindgen_export);
            const len1 = WASM_VECTOR_LEN;
            const ptr2 = passStringToWasm0(device, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            const len2 = WASM_VECTOR_LEN;
            wasm.boardapp_vault_publish(retptr, this.__wbg_ptr, ptr0, len0, ptr1, len1, ptr2, len2, until);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var r2 = getDataViewMemory0().getInt32(retptr + 4 * 2, true);
            if (r2) {
                throw takeObject(r1);
            }
            return takeObject(r0);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
}
if (Symbol.dispose) BoardApp.prototype[Symbol.dispose] = BoardApp.prototype.free;

export class ChannelApp {
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        ChannelAppFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_channelapp_free(ptr, 0);
    }
    /**
     * Joins the older posts of channel `index` from a CAR (fetched from a host that holds
     * them, or a backup); still-missing ones stay missing. Resolves the view's `missing`.
     * @param {number} index
     * @param {Uint8Array} car_bytes
     * @returns {number}
     */
    backfill(index, car_bytes) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            const ptr0 = passArray8ToWasm0(car_bytes, wasm.__wbindgen_export);
            const len0 = WASM_VECTOR_LEN;
            wasm.channelapp_backfill(retptr, this.__wbg_ptr, index, ptr0, len0);
            var r0 = getDataViewMemory0().getFloat64(retptr + 8 * 0, true);
            var r2 = getDataViewMemory0().getInt32(retptr + 4 * 2, true);
            var r3 = getDataViewMemory0().getInt32(retptr + 4 * 3, true);
            if (r3) {
                throw takeObject(r2);
            }
            return r0;
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * Channel `index`'s whole CAR (store, export, Kubo import).
     * @param {number} index
     * @returns {Uint8Array}
     */
    car(index) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.channelapp_car(retptr, this.__wbg_ptr, index);
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
     * The IPNS name of channel `index` of the signed-in identity (`k51…`).
     * @param {number} index
     * @returns {string}
     */
    channel_name(index) {
        let deferred2_0;
        let deferred2_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.channelapp_channel_name(retptr, this.__wbg_ptr, index);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var r2 = getDataViewMemory0().getInt32(retptr + 4 * 2, true);
            var r3 = getDataViewMemory0().getInt32(retptr + 4 * 3, true);
            var ptr1 = r0;
            var len1 = r1;
            if (r3) {
                ptr1 = 0; len1 = 0;
                throw takeObject(r2);
            }
            deferred2_0 = ptr1;
            deferred2_1 = len1;
            return getStringFromWasm0(ptr1, len1);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
            wasm.__wbindgen_export5(deferred2_0, deferred2_1, 1);
        }
    }
    /**
     * The onion address of channel `index` (`<56 chars>.onion`), derived like its keys: the
     * same on every device of the identity, so a device can read what another one serves.
     * @param {number} index
     * @returns {string}
     */
    channel_onion(index) {
        let deferred2_0;
        let deferred2_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.channelapp_channel_onion(retptr, this.__wbg_ptr, index);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var r2 = getDataViewMemory0().getInt32(retptr + 4 * 2, true);
            var r3 = getDataViewMemory0().getInt32(retptr + 4 * 3, true);
            var ptr1 = r0;
            var len1 = r1;
            if (r3) {
                ptr1 = 0; len1 = 0;
                throw takeObject(r2);
            }
            deferred2_0 = ptr1;
            deferred2_1 = len1;
            return getStringFromWasm0(ptr1, len1);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
            wasm.__wbindgen_export5(deferred2_0, deferred2_1, 1);
        }
    }
    /**
     * A new channel `index` (§D.3: use a separate identity for channels). Replaces nothing
     * stored: the page refuses when this channel already exists in its store.
     * @param {number} index
     * @param {string} title
     * @param {string} about
     */
    create(index, title, about) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            const ptr0 = passStringToWasm0(title, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            const len0 = WASM_VECTOR_LEN;
            const ptr1 = passStringToWasm0(about, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            const len1 = WASM_VECTOR_LEN;
            wasm.channelapp_create(retptr, this.__wbg_ptr, index, ptr0, len0, ptr1, len1);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            if (r1) {
                throw takeObject(r0);
            }
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * @param {number} index
     * @param {number} seq
     */
    delete(index, seq) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.channelapp_delete(retptr, this.__wbg_ptr, index, seq);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            if (r1) {
                throw takeObject(r0);
            }
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * The page signed out of channels or switched identity (also done by `bind` and
     * `sign_in` when the identity changes, M-1).
     */
    forget() {
        wasm.channelapp_forget(this.__wbg_ptr);
    }
    /**
     * @returns {string}
     */
    label() {
        let deferred1_0;
        let deferred1_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.channelapp_label(retptr, this.__wbg_ptr);
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
     * Mirrors a verified channel (§D.7.1) on this tab's onion for it. `seed` (32 bytes, kept
     * by the page per channel) makes the mirror's address stable across visits, so the owner
     * can sign it into the mirror list. Mirroring the same channel again (a newer version)
     * updates what is served, on the same address. Returns `<56 chars>.onion`.
     * @param {Reading} reading
     * @param {Uint8Array} seed
     * @returns {string}
     */
    mirror(reading, seed) {
        let deferred3_0;
        let deferred3_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            _assertClass(reading, Reading);
            const ptr0 = passArray8ToWasm0(seed, wasm.__wbindgen_export);
            const len0 = WASM_VECTOR_LEN;
            wasm.channelapp_mirror(retptr, this.__wbg_ptr, reading.__wbg_ptr, ptr0, len0);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var r2 = getDataViewMemory0().getInt32(retptr + 4 * 2, true);
            var r3 = getDataViewMemory0().getInt32(retptr + 4 * 3, true);
            var ptr2 = r0;
            var len2 = r1;
            if (r3) {
                ptr2 = 0; len2 = 0;
                throw takeObject(r2);
            }
            deferred3_0 = ptr2;
            deferred3_1 = len2;
            return getStringFromWasm0(ptr2, len2);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
            wasm.__wbindgen_export5(deferred3_0, deferred3_1, 1);
        }
    }
    constructor() {
        const ret = wasm.channelapp_new();
        this.__wbg_ptr = ret;
        ChannelAppFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
    /**
     * Reopens channel `index` from its stored CAR and record (the page's store, or an
     * imported backup). A record older than 7 days is re-signed (§D.5.3).
     * @param {number} index
     * @param {Uint8Array} car_bytes
     * @param {Uint8Array} record
     */
    open(index, car_bytes, record) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            const ptr0 = passArray8ToWasm0(car_bytes, wasm.__wbindgen_export);
            const len0 = WASM_VECTOR_LEN;
            const ptr1 = passArray8ToWasm0(record, wasm.__wbindgen_export);
            const len1 = WASM_VECTOR_LEN;
            wasm.channelapp_open(retptr, this.__wbg_ptr, index, ptr0, len0, ptr1, len1);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            if (r1) {
                throw takeObject(r0);
            }
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * Indices of the open channels.
     * @returns {Uint32Array}
     */
    open_channels() {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.channelapp_open_channels(retptr, this.__wbg_ptr);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var v1 = getArrayU32FromWasm0(r0, r1).slice();
            wasm.__wbindgen_export5(r0, r1 * 4, 4);
            return v1;
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * Publishes a post (≤ 4 KiB) in channel `index`; `reply` = the `seq` it answers, or 0.
     * @param {number} index
     * @param {string} body
     * @param {number} reply
     */
    post(index, body, reply) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            const ptr0 = passStringToWasm0(body, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            const len0 = WASM_VECTOR_LEN;
            wasm.channelapp_post(retptr, this.__wbg_ptr, index, ptr0, len0, reply);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            if (r1) {
                throw takeObject(r0);
            }
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * Whether the onion service at `onion` (a followed channel's or board's owner) accepts a
     * stream, i.e. its host is online. Nothing is sent: the stream is dropped once open. The
     * second try runs on a fresh circuit (a host that restarted has new introduction points).
     * Resolves to a bool; only a malformed address rejects.
     * @param {string} onion
     * @returns {Promise<any>}
     */
    probe(onion) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            const ptr0 = passStringToWasm0(onion, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            const len0 = WASM_VECTOR_LEN;
            wasm.channelapp_probe(retptr, this.__wbg_ptr, ptr0, len0);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var r2 = getDataViewMemory0().getInt32(retptr + 4 * 2, true);
            if (r2) {
                throw takeObject(r1);
            }
            return takeObject(r0);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * Publishes channel `index`'s record to the IPFS routing network (§D.5.2, optional):
     * `PUT https://delegated-ipfs.dev/routing/v1/ipns/<name>` **through a Tor exit**, so the
     * owner stays hidden. It only matters if some IPFS node holds the content (a follower's
     * Kubo mirror). `host`/`extra_root`: the lab's stand-in; the page passes the real host.
     * @param {number} index
     * @param {string} host
     * @param {Uint8Array} extra_root
     * @returns {Promise<any>}
     */
    publish_ipfs(index, host, extra_root) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            const ptr0 = passStringToWasm0(host, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            const len0 = WASM_VECTOR_LEN;
            const ptr1 = passArray8ToWasm0(extra_root, wasm.__wbindgen_export);
            const len1 = WASM_VECTOR_LEN;
            wasm.channelapp_publish_ipfs(retptr, this.__wbg_ptr, index, ptr0, len0, ptr1, len1);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var r2 = getDataViewMemory0().getInt32(retptr + 4 * 2, true);
            if (r2) {
                throw takeObject(r1);
            }
            return takeObject(r0);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * Whether readers can reach channel `index`'s onion yet (see `Service::reach`): empty if it
     * is not served.
     * @param {number} index
     * @returns {string}
     */
    reach(index) {
        let deferred1_0;
        let deferred1_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.channelapp_reach(retptr, this.__wbg_ptr, index);
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
     * Reads channel `name` over Tor from the first onion (comma-separated: owner, mirrors) that
     * serves a valid state; `min_seq` is the reader's high-water mark (§D.8). Resolves to the
     * JSON view plus the raw record and CAR (for a mirror or a local copy).
     * @param {string} name
     * @param {string} onions
     * @param {number} min_seq
     * @returns {Promise<any>}
     */
    read(name, onions, min_seq) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            const ptr0 = passStringToWasm0(name, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            const len0 = WASM_VECTOR_LEN;
            const ptr1 = passStringToWasm0(onions, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            const len1 = WASM_VECTOR_LEN;
            wasm.channelapp_read(retptr, this.__wbg_ptr, ptr0, len0, ptr1, len1, min_seq);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var r2 = getDataViewMemory0().getInt32(retptr + 4 * 2, true);
            if (r2) {
                throw takeObject(r1);
            }
            return takeObject(r0);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * Channel `index`'s current signed record.
     * @param {number} index
     * @returns {Uint8Array}
     */
    record(index) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.channelapp_record(retptr, this.__wbg_ptr, index);
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
     * The root CID (`bafy…`) a record names, after verifying it for `name` (a public
     * gateway read: the page fetches the record, then the CAR of this root).
     * @param {string} name
     * @param {Uint8Array} record
     * @returns {string}
     */
    record_root(name, record) {
        let deferred4_0;
        let deferred4_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            const ptr0 = passStringToWasm0(name, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            const len0 = WASM_VECTOR_LEN;
            const ptr1 = passArray8ToWasm0(record, wasm.__wbindgen_export);
            const len1 = WASM_VECTOR_LEN;
            wasm.channelapp_record_root(retptr, this.__wbg_ptr, ptr0, len0, ptr1, len1);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var r2 = getDataViewMemory0().getInt32(retptr + 4 * 2, true);
            var r3 = getDataViewMemory0().getInt32(retptr + 4 * 3, true);
            var ptr3 = r0;
            var len3 = r1;
            if (r3) {
                ptr3 = 0; len3 = 0;
                throw takeObject(r2);
            }
            deferred4_0 = ptr3;
            deferred4_1 = len3;
            return getStringFromWasm0(ptr3, len3);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
            wasm.__wbindgen_export5(deferred4_0, deferred4_1, 1);
        }
    }
    /**
     * Stops writing here: the open channels close and their onions go down (another device
     * holds the lease, §D.11.3 step 5). The store keeps them for reading.
     */
    release() {
        wasm.channelapp_release(this.__wbg_ptr);
    }
    /**
     * Continues channel `index` from the vault, without its blocks (§D.11.3 step 4): the
     * manifest is re-signed, new posts go on top of the older chain, which joins when a host
     * holding it is found (`backfill`).
     * @param {number} index
     */
    resume(index) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.channelapp_resume(retptr, this.__wbg_ptr, index);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            if (r1) {
                throw takeObject(r0);
            }
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * Serves channel `index` on its own onion address (§D.2); returns `<56 chars>.onion`. A
     * channel already online keeps its address (and serves its latest version).
     * @param {number} index
     * @returns {string}
     */
    serve(index) {
        let deferred2_0;
        let deferred2_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.channelapp_serve(retptr, this.__wbg_ptr, index);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var r2 = getDataViewMemory0().getInt32(retptr + 4 * 2, true);
            var r3 = getDataViewMemory0().getInt32(retptr + 4 * 3, true);
            var ptr1 = r0;
            var len1 = r1;
            if (r3) {
                ptr1 = 0; len1 = 0;
                throw takeObject(r2);
            }
            deferred2_0 = ptr1;
            deferred2_1 = len1;
            return getStringFromWasm0(ptr1, len1);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
            wasm.__wbindgen_export5(deferred2_0, deferred2_1, 1);
        }
    }
    /**
     * Signs the mirror list (comma-separated onion addresses) into the manifest (§D.7.1).
     * @param {number} index
     * @param {string} csv
     */
    set_mirrors(index, csv) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            const ptr0 = passStringToWasm0(csv, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            const len0 = WASM_VECTOR_LEN;
            wasm.channelapp_set_mirrors(retptr, this.__wbg_ptr, index, ptr0, len0);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            if (r1) {
                throw takeObject(r0);
            }
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * Opens a key file (the passphrase buffer is wiped). Only the seed is kept: channel keys
     * are derived from it and nothing of the chat identity is used (§D.3).
     * @param {Uint8Array} blob
     * @param {Uint8Array} pass
     */
    sign_in(blob, pass) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            const ptr0 = passArray8ToWasm0(blob, wasm.__wbindgen_export);
            const len0 = WASM_VECTOR_LEN;
            var ptr1 = passArray8ToWasm0(pass, wasm.__wbindgen_export);
            var len1 = WASM_VECTOR_LEN;
            wasm.channelapp_sign_in(retptr, this.__wbg_ptr, ptr0, len0, ptr1, len1, addHeapObject(pass));
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            if (r1) {
                throw takeObject(r0);
            }
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * @returns {Uint8Array}
     */
    tor_cache() {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.channelapp_tor_cache(retptr, this.__wbg_ptr);
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
     * @param {string} level
     */
    tor_log(level) {
        const ptr0 = passStringToWasm0(level, wasm.__wbindgen_export, wasm.__wbindgen_export2);
        const len0 = WASM_VECTOR_LEN;
        wasm.channelapp_tor_log(this.__wbg_ptr, ptr0, len0);
    }
    /**
     * As the chat's Tor mode (`tor.html`): Snowflake bridge lines (Appendix F.2), NAT hint, lab
     * network (empty = real Tor), directory snapshot for a warm start.
     * @param {string} bridges
     * @param {string} nat
     * @param {string} network_toml
     * @param {Uint8Array} cache
     * @returns {Promise<any>}
     */
    tor_start(bridges, nat, network_toml, cache) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            const ptr0 = passStringToWasm0(bridges, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            const len0 = WASM_VECTOR_LEN;
            const ptr1 = passStringToWasm0(nat, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            const len1 = WASM_VECTOR_LEN;
            const ptr2 = passStringToWasm0(network_toml, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            const len2 = WASM_VECTOR_LEN;
            const ptr3 = passArray8ToWasm0(cache, wasm.__wbindgen_export);
            const len3 = WASM_VECTOR_LEN;
            wasm.channelapp_tor_start(retptr, this.__wbg_ptr, ptr0, len0, ptr1, len1, ptr2, len2, ptr3, len3);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var r2 = getDataViewMemory0().getInt32(retptr + 4 * 2, true);
            if (r2) {
                throw takeObject(r1);
            }
            return takeObject(r0);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * @returns {string}
     */
    tor_status() {
        let deferred1_0;
        let deferred1_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.channelapp_tor_status(retptr, this.__wbg_ptr);
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
     * The vault as last fetched or published (JSON), "" before either.
     * @returns {string}
     */
    vault() {
        let deferred1_0;
        let deferred1_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.channelapp_vault(retptr, this.__wbg_ptr);
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
     * Fetches the vault record through a Tor exit (`GET https://<host>/routing/v1/ipns/<vault
     * name>`), verifies and opens it. Resolves to the vault as JSON ([`vault_json`]), or to ""
     * when there is none (404: never published, or forgotten by the DHT). A record older than
     * one seen before is ignored (the newer one stays); an equal one replaces ours.
     * @param {string} host
     * @param {Uint8Array} extra_root
     * @returns {Promise<any>}
     */
    vault_fetch(host, extra_root) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            const ptr0 = passStringToWasm0(host, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            const len0 = WASM_VECTOR_LEN;
            const ptr1 = passArray8ToWasm0(extra_root, wasm.__wbindgen_export);
            const len1 = WASM_VECTOR_LEN;
            wasm.channelapp_vault_fetch(retptr, this.__wbg_ptr, ptr0, len0, ptr1, len1);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var r2 = getDataViewMemory0().getInt32(retptr + 4 * 2, true);
            if (r2) {
                throw takeObject(r1);
            }
            return takeObject(r0);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * The highest vault sequence this device saw on an earlier visit (the page keeps it): an
     * older vault record is refused from now on (§D.11.5, security audit M-2).
     * @param {number} seq
     */
    vault_floor(seq) {
        wasm.channelapp_vault_floor(this.__wbg_ptr, seq);
    }
    /**
     * The vault's IPNS name (`k51…`) of the signed-in identity.
     * @returns {string}
     */
    vault_name() {
        let deferred2_0;
        let deferred2_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.channelapp_vault_name(retptr, this.__wbg_ptr);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var r2 = getDataViewMemory0().getInt32(retptr + 4 * 2, true);
            var r3 = getDataViewMemory0().getInt32(retptr + 4 * 3, true);
            var ptr1 = r0;
            var len1 = r1;
            if (r3) {
                ptr1 = 0; len1 = 0;
                throw takeObject(r2);
            }
            deferred2_0 = ptr1;
            deferred2_1 = len1;
            return getStringFromWasm0(ptr1, len1);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
            wasm.__wbindgen_export5(deferred2_0, deferred2_1, 1);
        }
    }
    /**
     * Publishes the vault (the open channels, plus the channels of the last vault that are not
     * open here) with the writer lease `{device (32 hex), until}` through a Tor exit (`PUT`, as
     * `publish_ipfs`). Resolves to the new sequence.
     * @param {string} host
     * @param {Uint8Array} extra_root
     * @param {string} device
     * @param {number} until
     * @returns {Promise<any>}
     */
    vault_publish(host, extra_root, device, until) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            const ptr0 = passStringToWasm0(host, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            const len0 = WASM_VECTOR_LEN;
            const ptr1 = passArray8ToWasm0(extra_root, wasm.__wbindgen_export);
            const len1 = WASM_VECTOR_LEN;
            const ptr2 = passStringToWasm0(device, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            const len2 = WASM_VECTOR_LEN;
            wasm.channelapp_vault_publish(retptr, this.__wbg_ptr, ptr0, len0, ptr1, len1, ptr2, len2, until);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var r2 = getDataViewMemory0().getInt32(retptr + 4 * 2, true);
            if (r2) {
                throw takeObject(r1);
            }
            return takeObject(r0);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * The vault sequence this tab saw or published (0: none).
     * @returns {number}
     */
    vault_seq() {
        const ret = wasm.channelapp_vault_seq(this.__wbg_ptr);
        return ret;
    }
    /**
     * Verifies a record and a CAR fetched by the page (a public gateway, an imported file).
     * @param {string} name
     * @param {Uint8Array} record
     * @param {Uint8Array} car_bytes
     * @param {number} min_seq
     * @returns {Reading}
     */
    verify(name, record, car_bytes, min_seq) {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            const ptr0 = passStringToWasm0(name, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            const len0 = WASM_VECTOR_LEN;
            const ptr1 = passArray8ToWasm0(record, wasm.__wbindgen_export);
            const len1 = WASM_VECTOR_LEN;
            const ptr2 = passArray8ToWasm0(car_bytes, wasm.__wbindgen_export);
            const len2 = WASM_VECTOR_LEN;
            wasm.channelapp_verify(retptr, this.__wbg_ptr, ptr0, len0, ptr1, len1, ptr2, len2, min_seq);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var r2 = getDataViewMemory0().getInt32(retptr + 4 * 2, true);
            if (r2) {
                throw takeObject(r1);
            }
            return Reading.__wrap(r0);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * Channel `index` as JSON (see [`json::view`]); empty if it is not open.
     * @param {number} index
     * @returns {string}
     */
    view(index) {
        let deferred1_0;
        let deferred1_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.channelapp_view(retptr, this.__wbg_ptr, index);
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
}
if (Symbol.dispose) ChannelApp.prototype[Symbol.dispose] = ChannelApp.prototype.free;

/**
 * An open reply box: the board, its `/pow` answer and a fresh poster key (RAM only).
 */
export class Draft {
    static __wrap(ptr) {
        const obj = Object.create(Draft.prototype);
        obj.__wbg_ptr = ptr;
        DraftFinalization.register(obj, obj.__wbg_ptr, obj);
        return obj;
    }
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        DraftFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_draft_free(ptr, 0);
    }
    /**
     * @returns {number}
     */
    get effort_now() {
        const ret = wasm.draft_effort_now(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * What a Worker solves: `{name, seed, thread, k, kind, effort}` (the page adds a random
     * starting nonce per Worker).
     * @returns {object}
     */
    params() {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.draft_params(retptr, this.__wbg_ptr);
            var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
            var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
            var r2 = getDataViewMemory0().getInt32(retptr + 4 * 2, true);
            if (r2) {
                throw takeObject(r1);
            }
            return takeObject(r0);
        } finally {
            wasm.__wbindgen_add_to_stack_pointer(16);
        }
    }
    /**
     * @returns {boolean}
     */
    get paused() {
        const ret = wasm.draft_paused(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * The board's switches as `/pow` said: `{trips_only, approved_only, premod}` (JSON).
     * @returns {string}
     */
    get switches() {
        let deferred1_0;
        let deferred1_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.draft_switches(retptr, this.__wbg_ptr);
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
     * @returns {boolean}
     */
    get threads_open() {
        const ret = wasm.draft_threads_open(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * The trip this draft posts under (`!` + 16 characters), or "".
     * @returns {string}
     */
    get trip() {
        let deferred1_0;
        let deferred1_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.draft_trip(retptr, this.__wbg_ptr);
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
}
if (Symbol.dispose) Draft.prototype[Symbol.dispose] = Draft.prototype.free;

/**
 * A verified channel as handed to the page: the JSON view, and the bytes it came from.
 */
export class Reading {
    static __wrap(ptr) {
        const obj = Object.create(Reading.prototype);
        obj.__wbg_ptr = ptr;
        ReadingFinalization.register(obj, obj.__wbg_ptr, obj);
        return obj;
    }
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        ReadingFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_reading_free(ptr, 0);
    }
    /**
     * @returns {Uint8Array}
     */
    car() {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.reading_car(retptr, this.__wbg_ptr);
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
     * @returns {string}
     */
    get json() {
        let deferred1_0;
        let deferred1_1;
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.reading_json(retptr, this.__wbg_ptr);
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
     * @returns {Uint8Array}
     */
    record() {
        try {
            const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
            wasm.reading_record(retptr, this.__wbg_ptr);
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
     * @returns {number}
     */
    get sequence() {
        const ret = wasm.reading_sequence(this.__wbg_ptr);
        return ret;
    }
}
if (Symbol.dispose) Reading.prototype[Symbol.dispose] = Reading.prototype.free;

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
        __wbg_call_1875a20c43a36133: function() { return handleError(function (arg0, arg1, arg2, arg3) {
            const ret = getObject(arg0).call(getObject(arg1), getObject(arg2), getObject(arg3));
            return addHeapObject(ret);
        }, arguments); },
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
        __wbg_draft_new: function(arg0) {
            const ret = Draft.__wrap(arg0);
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
                        return __wasm_bindgen_func_elem_6792(a, state0.b, arg0, arg1);
                    } finally {
                        state0.a = a;
                    }
                };
                getObject(arg0).forEach(cb0);
            } finally {
                state0.a = 0;
            }
        },
        __wbg_from_296ca31f8d0f1c52: function(arg0) {
            const ret = Array.from(getObject(arg0));
            return addHeapObject(ret);
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
        __wbg_get_6c896e0571ddae51: function(arg0, arg1) {
            const ret = getObject(arg0)[arg1 >>> 0];
            return addHeapObject(ret);
        },
        __wbg_get_unchecked_288889d017702237: function(arg0, arg1) {
            const ret = getObject(arg0)[arg1 >>> 0];
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
        __wbg_length_d4bdea10311bd9cf: function(arg0) {
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
                        return __wasm_bindgen_func_elem_6792_172(a, state0.b, arg0, arg1);
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
        __wbg_new_from_slice_9a868026ffa4208a: function(arg0, arg1) {
            const ret = new Uint8Array(getArrayU8FromWasm0(arg0, arg1));
            return addHeapObject(ret);
        },
        __wbg_new_typed_b01cb72a8af741a3: function(arg0, arg1) {
            try {
                var state0 = {a: arg0, b: arg1};
                var cb0 = (arg0, arg1) => {
                    const a = state0.a;
                    state0.a = 0;
                    try {
                        return __wasm_bindgen_func_elem_6792_173(a, state0.b, arg0, arg1);
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
        __wbg_new_with_configuration_c6d3ab433d0adb73: function() { return handleError(function (arg0) {
            const ret = new RTCPeerConnection(getObject(arg0));
            return addHeapObject(ret);
        }, arguments); },
        __wbg_new_with_length_3da0ad195f6f63ba: function(arg0) {
            const ret = new Uint8Array(arg0 >>> 0);
            return addHeapObject(ret);
        },
        __wbg_new_with_length_469fcc27bd71672e: function(arg0) {
            const ret = new Array(arg0 >>> 0);
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
        __wbg_of_20798cb14708764f: function(arg0, arg1) {
            const ret = Array.of(getObject(arg0), getObject(arg1));
            return addHeapObject(ret);
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
        __wbg_reading_new: function(arg0) {
            const ret = Reading.__wrap(arg0);
            return addHeapObject(ret);
        },
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
        __wbg_set_bea140a88be9b277: function(arg0, arg1, arg2) {
            getObject(arg0)[arg1 >>> 0] = takeObject(arg2);
        },
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
            // Cast intrinsic for `Closure(Closure { owned: true, function: Function { arguments: [Externref], shim_idx: 847, ret: Result(Unit), inner_ret: Some(Result(Unit)) }, mutable: true }) -> Externref`.
            const ret = makeMutClosure(arg0, arg1, __wasm_bindgen_func_elem_6757);
            return addHeapObject(ret);
        },
        __wbindgen_generic_0000000000000002: function(arg0, arg1) {
            // Cast intrinsic for `Closure(Closure { owned: true, function: Function { arguments: [NamedExternref("MessageEvent")], shim_idx: 647, ret: Unit, inner_ret: Some(Unit) }, mutable: true }) -> Externref`.
            const ret = makeMutClosure(arg0, arg1, __wasm_bindgen_func_elem_4851);
            return addHeapObject(ret);
        },
        __wbindgen_generic_0000000000000003: function(arg0, arg1) {
            // Cast intrinsic for `Closure(Closure { owned: true, function: Function { arguments: [], shim_idx: 384, ret: Unit, inner_ret: Some(Unit) }, mutable: true }) -> Externref`.
            const ret = makeMutClosure(arg0, arg1, __wasm_bindgen_func_elem_3130);
            return addHeapObject(ret);
        },
        __wbindgen_generic_0000000000000004: function(arg0) {
            // Cast intrinsic for `F64 -> Externref`.
            const ret = arg0;
            return addHeapObject(ret);
        },
        __wbindgen_generic_0000000000000005: function(arg0, arg1) {
            // Cast intrinsic for `Ref(Slice(U8)) -> NamedExternref("Uint8Array")`.
            const ret = getArrayU8FromWasm0(arg0, arg1);
            return addHeapObject(ret);
        },
        __wbindgen_generic_0000000000000006: function(arg0, arg1) {
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

function __wasm_bindgen_func_elem_3130(arg0, arg1) {
    wasm.__wasm_bindgen_func_elem_3130(arg0, arg1);
}

function __wasm_bindgen_func_elem_4851(arg0, arg1, arg2) {
    wasm.__wasm_bindgen_func_elem_4851(arg0, arg1, addHeapObject(arg2));
}

function __wasm_bindgen_func_elem_6792(arg0, arg1, arg2, arg3) {
    wasm.__wasm_bindgen_func_elem_6792(arg0, arg1, addHeapObject(arg2), addHeapObject(arg3));
}

function __wasm_bindgen_func_elem_6792_172(arg0, arg1, arg2, arg3) {
    wasm.__wasm_bindgen_func_elem_6792_172(arg0, arg1, addHeapObject(arg2), addHeapObject(arg3));
}

function __wasm_bindgen_func_elem_6792_173(arg0, arg1, arg2, arg3) {
    wasm.__wasm_bindgen_func_elem_6792_173(arg0, arg1, addHeapObject(arg2), addHeapObject(arg3));
}

function __wasm_bindgen_func_elem_6757(arg0, arg1, arg2) {
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        wasm.__wasm_bindgen_func_elem_6757(retptr, arg0, arg1, addHeapObject(arg2));
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
const BoardAppFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_boardapp_free(ptr, 1));
const ChannelAppFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_channelapp_free(ptr, 1));
const DraftFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_draft_free(ptr, 1));
const ReadingFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_reading_free(ptr, 1));

function addHeapObject(obj) {
    if (heap_next === heap.length) heap.push(heap.length + 1);
    const idx = heap_next;
    heap_next = heap[idx];

    heap[idx] = obj;
    return idx;
}

function _assertClass(instance, klass) {
    if (!(instance instanceof klass)) {
        throw new Error(`expected instance of ${klass.name}`);
    }
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

function getArrayU32FromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return getUint32ArrayMemory0().subarray(ptr / 4, ptr / 4 + len);
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

let cachedFloat64ArrayMemory0 = null;
function getFloat64ArrayMemory0() {
    if (cachedFloat64ArrayMemory0 === null || cachedFloat64ArrayMemory0.byteLength === 0) {
        cachedFloat64ArrayMemory0 = new Float64Array(wasm.memory.buffer);
    }
    return cachedFloat64ArrayMemory0;
}

function getStringFromWasm0(ptr, len) {
    return decodeText(ptr >>> 0, len);
}

let cachedUint32ArrayMemory0 = null;
function getUint32ArrayMemory0() {
    if (cachedUint32ArrayMemory0 === null || cachedUint32ArrayMemory0.byteLength === 0) {
        cachedUint32ArrayMemory0 = new Uint32Array(wasm.memory.buffer);
    }
    return cachedUint32ArrayMemory0;
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

function passArrayF64ToWasm0(arg, malloc) {
    const ptr = malloc(arg.length * 8, 8) >>> 0;
    getFloat64ArrayMemory0().set(arg, ptr / 8);
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
    cachedFloat64ArrayMemory0 = null;
    cachedUint32ArrayMemory0 = null;
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
