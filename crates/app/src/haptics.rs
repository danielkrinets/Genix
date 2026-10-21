//! Trackpad haptic ticks (Mac only; elsewhere a no-op): one light click per step while a value is scrubbed.

#[cfg(target_os = "macos")]
pub fn tick() {
    use objc2_app_kit::{NSHapticFeedbackManager, NSHapticFeedbackPattern, NSHapticFeedbackPerformanceTime, NSHapticFeedbackPerformer};
    unsafe {
        let performer = NSHapticFeedbackManager::defaultPerformer();
        performer.performFeedbackPattern_performanceTime(NSHapticFeedbackPattern::Alignment, NSHapticFeedbackPerformanceTime::Default);
    }
}

#[cfg(not(target_os = "macos"))]
pub fn tick() {}
