use super::super::*;

impl Vivling {
    pub(crate) fn update_existing<F>(&mut self, f: F) -> Result<String, String>
    where
        F: FnOnce(&mut VivlingState) -> String,
    {
        self.ensure_hatched()?;
        let message = {
            let Some(state) = self.state.as_mut() else {
                return Err("No Vivling yet. Use /vivling hatch.".to_string());
            };
            state.apply_decay(Utc::now());
            f(state)
        };
        self.save_state().map_err(|err| err.to_string())?;
        Ok(message)
    }

    pub(crate) fn update_existing_value<F, T>(&mut self, f: F) -> Result<T, String>
    where
        F: FnOnce(&mut VivlingState) -> T,
    {
        self.ensure_hatched()?;
        let value = {
            let Some(state) = self.state.as_mut() else {
                return Err("No Vivling yet. Use /vivling hatch.".to_string());
            };
            state.apply_decay(Utc::now());
            f(state)
        };
        self.save_state().map_err(|err| err.to_string())?;
        Ok(value)
    }

    pub(crate) fn update_existing_result<F>(&mut self, f: F) -> Result<String, String>
    where
        F: FnOnce(&mut VivlingState) -> Result<String, String>,
    {
        self.ensure_hatched()?;
        let message = {
            let Some(state) = self.state.as_mut() else {
                return Err("No Vivling yet. Use /vivling hatch.".to_string());
            };
            state.apply_decay(Utc::now());
            f(state)?
        };
        self.save_state().map_err(|err| err.to_string())?;
        Ok(message)
    }
}
