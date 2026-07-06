use super::*;

impl<T: Config> Pallet<T> {
    pub fn do_try_associate_hotkey(
        coldkey: &T::AccountId,
        hotkey: &T::AccountId,
    ) -> DispatchResult {
        // Ensure the hotkey is not already associated with a coldkey
        Self::create_account_if_non_existent(coldkey, hotkey)?;

        Ok(())
    }
}
