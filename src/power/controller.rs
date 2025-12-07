use super::{PowerControllerError, PowerControllerResult};
use crate::board::BoostEnPin;
use bq24296m::{
    BatteryLowVoltageThreshold, BatteryRechargeThreshold, BoostCurrentLimit, BoostHotThreshold,
    ChargeTimer, ConfigurationRegisters, InputCurrentLimit, NewFaultRegister,
    PowerOnConfigurationRegister, StatusRegisters, SystemStatusRegister,
    ThermalRegulationThreshold, WatchdogTimer, BQ24296,
};
use bitfields::bitfield;
use defmt::{debug, Format};
use embedded_hal::i2c::I2c;
use esp_hal::gpio::*;
use pcf857x::Pcf8574;

pub struct PowerControllerIO<I2C: I2c> {
    pub charger_i2c: I2C,
    pub pcf8574_i2c: I2C,
    pub boost_converter_enable: BoostEnPin,
}

pub struct PowerControllerConfig {
    pub precharge_current: u32,
    pub charging_current: u32,
    pub termination_current: u32,
    pub charging_voltage: u32,
    pub charge_timer: Option<ChargeTimer>,
    pub battary_recharge_threshold: BatteryRechargeThreshold,
    pub battery_low_voltage: BatteryLowVoltageThreshold,

    pub input_current: InputCurrentLimit,
    pub input_voltage: u32,
    pub sys_min_voltage: u32,

    pub boost_voltage: u32,
    pub boost_current_limit: BoostCurrentLimit,
    pub boost_hot_threshold: BoostHotThreshold,
    pub boost_cold_threshold_m20: bool,

    pub i2c_watchdog_timer: WatchdogTimer,
    pub thermal_regulation_threshold: ThermalRegulationThreshold,
    pub enable_charge_fault_int: bool,
    pub enable_battery_fault_int: bool,
}

#[bitfield(u8, from = true)]
#[derive(Clone, Copy, Format)]
pub struct ExpanderReg {
    #[bits(1)]
    chr_en: u8,

    #[bits(1)]
    chr_otg: u8,

    #[bits(1)]
    _unused_P2: u8,

    #[bits(1)]
    chr_psel: u8,

    #[bits(1)]
    vbus_flg: u8,

    #[bits(1)]
    vbus_enable: u8,

    #[bits(1)]
    vbus_present: u8,

    #[bits(1)]
    dc_jack_present: u8,
}

#[derive(Clone, Copy, Format, Debug)]
pub struct ExpanderStatus {
    reg: ExpanderReg,
}

impl ExpanderStatus {
    // chr_en - active low
    pub fn set_chr_en(&mut self, enabled: bool) {
        self.reg.set_chr_en(if enabled { 0 } else { 1 });
    }

    pub fn chr_en(&self) -> bool {
        self.reg.chr_en() == 0
    }

    // chr_otg - active high
    pub fn set_chr_otg(&mut self, enabled: bool) {
        self.reg.set_chr_otg(if enabled { 1 } else { 0 });
    }

    pub fn chr_otg(&self) -> bool {
        self.reg.chr_otg() != 0
    }

    // chr_psel - active low
    pub fn set_chr_psel(&mut self, enabled: bool) {
        self.reg.set_chr_psel(if enabled { 0 } else { 1 });
    }

    pub fn chr_psel(&self) -> bool {
        self.reg.chr_psel() == 0
    }

    // chr_vbus_enable - active high
    pub fn set_vbus_enable(&mut self, enabled: bool) {
        self.reg.set_vbus_enable(if enabled { 1 } else { 0 });
    }

    pub fn vbus_enable(&self) -> bool {
        self.reg.vbus_enable() != 0
    }

    // vbus_flg - active low
    pub fn vbus_flg(&self) -> bool {
        self.reg.vbus_flg() == 0
    }

    // vbus_present - active low
    pub fn vbus_present(&self) -> bool {
        self.reg.vbus_present() == 0
    }

    // dc_jack_present - active low
    pub fn dc_jack_present(&self) -> bool {
        self.reg.dc_jack_present() == 0
    }
}

impl Into<u8> for ExpanderStatus {
    fn into(self) -> u8 {
        self.reg.into()
    }
}

impl From<u8> for ExpanderStatus {
    fn from(value: u8) -> Self {
        Self {
            reg: ExpanderReg::from(value),
        }
    }
}

#[derive(Debug, Format, Clone)]
pub struct PowerControllerStats {
    pub charger_status: SystemStatusRegister,
    pub charger_faults: NewFaultRegister,
    pub boost_enabled: bool,
    pub expander_status: ExpanderStatus,
}

impl PowerControllerStats {
    pub fn dump(&self) {
        debug!("PowerControllerStats:");

        let status = &self.charger_status;
        debug!("> Charger Status:");
        debug!("  VBUS Status: {:?}", status.get_vbus_status());
        debug!("  Charge Status: {:?}", status.get_charge_status());
        debug!("  DPM Active: {}", status.is_dpm_active());
        debug!("  Power Good: {}", status.is_power_good());
        debug!(
            "  Thermal Regulation Active: {}",
            status.is_thermal_regulation_active()
        );
        debug!(
            "  VSYS Regulation Active: {}",
            status.is_vsys_regulation_active()
        );

        let faults = &self.charger_faults;
        debug!("> Charger Faults:");
        debug!("  NTC Fault Status: {:?}", faults.get_ntc_fault_status());
        debug!("    - Cold Fault: {}", faults.is_ntc_cold_fault());
        debug!("    - Hot Fault: {}", faults.is_ntc_hot_fault());
        debug!("  Battery Fault: {}", faults.is_battery_fault());
        debug!(
            "  Charger Fault Status: {:?}",
            faults.get_charge_fault_status()
        );
        debug!("  OTG Fault: {}", faults.is_otg_fault());
        debug!("  Watchdog Fault: {}", faults.is_watchdog_fault());
        
        debug!("> Expander Status:");
        debug!("  Inputs:");
        debug!("    VBUS Present: {}", self.expander_status.vbus_present());
        debug!("    VBUS Flag: {}", self.expander_status.vbus_flg());
        debug!("    DC Jack Present: {}", self.expander_status.dc_jack_present());
        debug!("  Outputs:");
        debug!("    Charger Enable: {}", self.expander_status.chr_en());
        debug!("    Charger OTG: {}", self.expander_status.chr_otg());
        debug!("    Charger PSEL: {}", self.expander_status.chr_psel());
        debug!("    VBUS Enable: {}", self.expander_status.vbus_enable());
    }
}

impl Default for PowerControllerConfig {
    fn default() -> Self {
        Self {
            precharge_current: 512,
            charging_current: 1024,
            termination_current: 100,
            charging_voltage: 4100,
            charge_timer: Some(ChargeTimer::Hours8),
            battary_recharge_threshold: BatteryRechargeThreshold::mV_100,
            battery_low_voltage: BatteryLowVoltageThreshold::mV_3000,

            input_current: InputCurrentLimit::mA_1000,
            input_voltage: 4360,
            sys_min_voltage: 3700,

            boost_voltage: 4998,
            boost_current_limit: BoostCurrentLimit::mA_1000,
            boost_hot_threshold: BoostHotThreshold::Celsius65,
            boost_cold_threshold_m20: true,

            i2c_watchdog_timer: WatchdogTimer::Seconds160,
            thermal_regulation_threshold: ThermalRegulationThreshold::Celsius80,
            enable_charge_fault_int: true,
            enable_battery_fault_int: true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PowerControllerMode {
    Passive,
    Charging,
    Otg,
}

pub struct PowerController<I2C: I2c> {
    config: PowerControllerConfig,
    mode: PowerControllerMode,
    charger: BQ24296<I2C>,
    expander: Pcf8574<I2C>,
    boost_converter_enable: Output<'static>,
}

impl<I2C: I2c> PowerController<I2C> {
    pub fn new(config: PowerControllerConfig, io: PowerControllerIO<I2C>) -> PowerControllerResult<Self, I2C> {
        let charger = BQ24296::new(io.charger_i2c);
        let address = pcf857x::SlaveAddr::Alternative(true, false, true);
        let expander = Pcf8574::new(io.pcf8574_i2c, address);
        let boost_pin = Output::new(
            io.boost_converter_enable,
            Level::Low,
            OutputConfig::default(),
        );

        let mut device = Self {
            config,
            mode: PowerControllerMode::Passive,
            charger,
            expander,
            boost_converter_enable: boost_pin,
        };

        device.setup_expander()?;
        device.write_charger_config()?;

        Ok(device)
    }

    fn setup_expander(&mut self) -> PowerControllerResult<(), I2C> {
        // Set chr_otg high by default
        let mut status = ExpanderStatus::from(0xFF);
        status.set_chr_otg(true);
        self.expander
            .set(status.into())
            .map_err(PowerControllerError::I2CExpanderError)
    }

    fn write_charger_config(&mut self) -> PowerControllerResult<(), I2C> {
        self.charger
            .transact(|regs: &mut ConfigurationRegisters| {
                regs.ISCR.set_hiz_enabled(false);
                regs.ISCR
                    .set_input_voltage_dpm_mV(self.config.input_voltage);
                regs.ISCR.set_input_current_limit(self.config.input_current);

                regs.POCR.reset_i2c_watchdog();
                regs.POCR
                    .set_system_min_bus_voltage_mV(self.config.sys_min_voltage);
                regs.POCR
                    .set_boost_current_limit(self.config.boost_current_limit);

                regs.CCCR
                    .set_charge_current_limit_mA(self.config.charging_current);
                if self.config.boost_cold_threshold_m20 {
                    regs.CCCR.set_boost_converter_low_temp_to_m20();
                } else {
                    regs.CCCR.set_boost_converter_low_temp_to_m10();
                }

                regs.PCTCCR
                    .set_precharge_current_mA(self.config.precharge_current);
                regs.PCTCCR
                    .set_termination_current_mA(self.config.termination_current);

                regs.CVCR
                    .set_charge_voltage_limit_mV(self.config.charging_voltage);
                regs.CVCR
                    .set_battery_low_voltage_threshold(self.config.battery_low_voltage);
                regs.CVCR
                    .set_battery_recharge_threshold(self.config.battary_recharge_threshold);

                regs.CTTCR.enable_termination();
                regs.CTTCR
                    .set_watchdog_timer(self.config.i2c_watchdog_timer);
                match self.config.charge_timer {
                    Some(dt) => {
                        regs.CTTCR.set_charge_timer(dt);
                        regs.CTTCR.enable_safety_timer();
                    }
                    None => regs.CTTCR.disable_safety_timer(),
                }

                regs.BVTRR.set_boost_voltage_mV(self.config.boost_voltage);
                regs.BVTRR
                    .set_boost_hot_temperature_threshold(self.config.boost_hot_threshold);
                regs.BVTRR
                    .set_thermal_regulation_threshold(self.config.thermal_regulation_threshold);

                regs.MOCR.enable_dpdm_detection();
                regs.MOCR.disable_timer_2x();
                regs.MOCR.enable_batfet();
                let int_mask = match (
                    self.config.enable_charge_fault_int,
                    self.config.enable_battery_fault_int,
                ) {
                    (false, false) => 0u8,
                    (false, true) => 1u8,
                    (true, false) => 2u8,
                    (true, true) => 3u8,
                };
                regs.MOCR.set_interrupt_mask(int_mask);
            })
            .map_err(PowerControllerError::I2cBusError)
    }

    pub fn reconfigure(&mut self, f: impl FnOnce(&mut PowerControllerConfig)) -> PowerControllerResult<(), I2C> {
        f(&mut self.config);
        self.write_charger_config()
    }

    pub fn switch_mode(&mut self, mode: PowerControllerMode, stats: &PowerControllerStats) -> PowerControllerResult<(), I2C> {
        let mut status = stats.expander_status;

        match mode {
            PowerControllerMode::Passive => {
                status.set_chr_en(false);
                status.set_vbus_enable(true);
                self.expander
                    .set(status.into())
                    .map_err(PowerControllerError::I2CExpanderError)?;
                self.charger
                    .transact(|r: &mut PowerOnConfigurationRegister| {
                        r.disable_charging();
                        r.disable_otg();
                    })
                    .map_err(PowerControllerError::I2cBusError)?;
            }
            PowerControllerMode::Charging => {
                status.set_chr_en(true);
                status.set_vbus_enable(true);
                self.expander
                    .set(status.into())
                    .map_err(PowerControllerError::I2CExpanderError)?;
                self.charger
                    .transact(|r: &mut PowerOnConfigurationRegister| {
                        r.enable_charging();
                        r.disable_otg();
                    })
                    .map_err(PowerControllerError::I2cBusError)?;
            }
            PowerControllerMode::Otg => {
                status.set_chr_en(false);
                status.set_vbus_enable(false);
                self.expander
                    .set(status.into())
                    .map_err(PowerControllerError::I2CExpanderError)?;
                self.charger
                    .transact(|r: &mut PowerOnConfigurationRegister| {
                        r.disable_charging();
                        r.enable_otg();
                    })
                    .map_err(PowerControllerError::I2cBusError)?;
            }
        }

        self.mode = mode;

        Ok(())
    }

    pub fn read_stats(&mut self) -> PowerControllerResult<PowerControllerStats, I2C> {
        let stats: StatusRegisters = self
            .charger
            .read()
            .map_err(PowerControllerError::I2cBusError)?;

        let expander_status = self.read_expander_status()?;

        Ok(PowerControllerStats {
            charger_status: stats.SSR,
            charger_faults: stats.NFR,
            boost_enabled: self.is_boost_converter_enabled(),
            expander_status,
        })
    }

    fn read_expander_status(&mut self) -> PowerControllerResult<ExpanderStatus, I2C> {
        // Read entire byte from PCF8574
        // Only read input pins: P4 (vbus_flg), P6 (vbus_present), P7 (dc_jack_present)
        use pcf857x::PinFlag;
        let input_pins = PinFlag::P4 | PinFlag::P6 | PinFlag::P7;
        let byte = self
            .expander
            .get(input_pins)
            .map_err(PowerControllerError::I2CExpanderError)?;
        Ok(ExpanderStatus::from(byte))
    }

    pub fn reset_watchdog(&mut self) -> PowerControllerResult<(), I2C> {
        self.charger
            .transact(|r: &mut PowerOnConfigurationRegister| {
                r.reset_i2c_watchdog();
            })
            .map_err(PowerControllerError::I2cBusError)?;

        Ok(())
    }

    pub fn get_mode(&self) -> &PowerControllerMode {
        &self.mode
    }

    pub fn enable_boost_converter(&mut self) {
        self.boost_converter_enable.set_high();
    }

    pub fn disable_boost_converter(&mut self) {
        self.boost_converter_enable.set_low();
    }

    pub fn is_boost_converter_enabled(&self) -> bool {
        self.boost_converter_enable.is_set_high()
    }

    pub fn enter_shipping_mode(&mut self, stats: &PowerControllerStats) -> PowerControllerResult<(), I2C> {
        self.switch_mode(PowerControllerMode::Charging, stats)?;

        self.charger
            .transact(|regs: &mut ConfigurationRegisters| {
                regs.CTTCR.set_watchdog_timer(WatchdogTimer::Disabled);
                regs.MOCR.disable_batfet();
            })
            .map_err(PowerControllerError::I2cBusError)?;

        Ok(())
    }
}
