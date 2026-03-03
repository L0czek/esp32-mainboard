use embedded_hal::i2c::I2c;
use pcf857x::Pcf8574;

pub struct FireTrigger<I2C: I2c> {
    expander: Pcf8574<I2C>,
    trigger_byte: u8,
}

impl<I2C: I2c> FireTrigger<I2C> {
    pub fn new(
        i2c: I2C,
        address: pcf857x::SlaveAddr,
        trigger_byte: u8,
    ) -> Result<Self, pcf857x::Error<I2C::Error>> {
        let mut expander = Pcf8574::new(i2c, address);
        expander.set(0xFF)?;
        Ok(Self {
            expander,
            trigger_byte,
        })
    }

    pub fn trigger(&mut self) -> Result<(), pcf857x::Error<I2C::Error>> {
        self.expander.set(self.trigger_byte)
    }

    pub fn abort(&mut self) -> Result<(), pcf857x::Error<I2C::Error>> {
        self.expander.set(0xFF)
    }
}
