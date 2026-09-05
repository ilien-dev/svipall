/// Contains the logic to implement PMTUD. Given a maximum supported MTU,
/// finds the PMTU between the given max and [`MIN_CLIENT_INITIAL_LEN`].
use crate::MIN_CLIENT_INITIAL_LEN;

#[derive(Default)]
pub struct Pmtud {
    /// The PMTU after the completion of PMTUD.
    /// Will be [`None`] if the PMTU is less than the minimum supported MTU.
    pmtu: Option<usize>,

    /// The current PMTUD probe size. Set to maximum_supported_mtu at
    /// initialization.
    probe_size: usize,

    /// The maximum supported MTU.
    maximum_supported_mtu: usize,

    /// The size of the smallest failed probe.
    smallest_failed_probe_size: Option<usize>,

    /// The size of the largest successful probe.
    largest_successful_probe_size: Option<usize>,

    /// Indicates if a PMTUD probe is in flight. Used to limit probes to 1/RTT.
    in_flight: bool,
}

impl Pmtud {
    /// Creates new PMTUD instance.
    pub fn new(maximum_supported_mtu: usize) -> Self {
        Self {
            maximum_supported_mtu,
            probe_size: maximum_supported_mtu,
            ..Default::default()
        }
    }

    /// Indicates whether probing should continue on the connection.
    ///
    /// Checks there are no probes in flight, that a PMTU has not been
    /// found, and that the minimum supported MTU has not been reached.
    pub fn should_probe(&self) -> bool {
        !self.in_flight
            && self.pmtu.is_none()
            && self.smallest_failed_probe_size != Some(MIN_CLIENT_INITIAL_LEN)
    }

    /// Sets the PMTUD probe size.
    fn set_probe_size(&mut self, probe_size: usize) {
        self.probe_size = std::cmp::min(probe_size, self.maximum_supported_mtu);
    }

    /// Returns the PMTUD probe size.
    pub fn get_probe_size(&self) -> usize {
        self.probe_size
    }

    /// Returns the largest successful PMTUD probe size if one exists, otherwise
    /// returns the minimum supported MTU.
    pub fn get_current_mtu(&self) -> usize {
        self.largest_successful_probe_size
            .unwrap_or(MIN_CLIENT_INITIAL_LEN)
    }

    /// Returns the PMTU.
    pub fn get_pmtu(&self) -> Option<usize> {
        self.pmtu
    }

    /// Selects PMTU probe size based on the binary search algorithm.
    ///
    /// Based on the Optimistic Binary algorithm defined in:
    /// Ref: <https://www.hb.fh-muenster.de/opus4/frontdoor/deliver/index/docId/14965/file/dplpmtudQuicPaper.pdf>
    fn update_probe_size(&mut self) {
        match (
            self.smallest_failed_probe_size,
            self.largest_successful_probe_size,
        ) {
            // Binary search between successful and failed probes
            (Some(failed_probe_size), Some(successful_probe_size)) => {
                // Something has changed along the path that invalidates
                // previous PMTUD probes. Restart PMTUD
                if failed_probe_size <= successful_probe_size {
                    warn!(
                        "Inconsistent PMTUD probing results. Restarting PMTUD. \
                        failed_probe_size: {failed_probe_size}, \
                        successful_probe_size: {successful_probe_size}",
                    );

                    return self.restart_pmtud();
                }

                // Found the PMTU
                if failed_probe_size - successful_probe_size <= 1 {
                    trace!("Found PMTU: {successful_probe_size}");

                    self.pmtu = Some(successful_probe_size);
                    self.probe_size = successful_probe_size
                } else {
                    self.probe_size = (successful_probe_size + failed_probe_size) / 2
                }
            }

            // With only failed probes, binary search between the smallest failed
            // probe and the minimum supported MTU
            (Some(failed_probe_size), None) => {
                self.probe_size = (MIN_CLIENT_INITIAL_LEN + failed_probe_size) / 2
            }

            // As the algorithm is optimistic in that the initial probe size
            // is the maximum supported MTU, then having only a successful probe
            // means the maximum supported MTU is <= PMTU
            (None, Some(successful_probe_size)) => {
                self.pmtu = Some(successful_probe_size);
                self.probe_size = successful_probe_size
            }

            // Use the initial probe size if no record of success/failures
            (None, None) => self.probe_size = self.maximum_supported_mtu,
        }
    }

    /// Sets whether a probe is currently in flight for this connection.
    pub fn set_in_flight(&mut self, in_flight: bool) {
        self.in_flight = in_flight;
    }

    /// Records a successful probe and returns the largest successful probe size
    pub fn successful_probe(&mut self, probe_size: usize) -> Option<usize> {
        self.largest_successful_probe_size = std::cmp::max(
            // make sure we don't exceed the maximum supported MTU
            Some(probe_size.min(self.maximum_supported_mtu)),
            self.largest_successful_probe_size,
        );

        self.update_probe_size();
        self.in_flight = false;

        self.largest_successful_probe_size
    }

    /// Records a failed probe
    pub fn failed_probe(&mut self, probe_size: usize) {
        // Treat errant probes as if they failed at the minimum supported MTU
        let probe_size = std::cmp::max(probe_size, MIN_CLIENT_INITIAL_LEN);

        // Check if we have one instance of a failed probe so that a min
        // comparison can be made otherwise if this is the first failed
        // probe just record it
        self.smallest_failed_probe_size = self
            .smallest_failed_probe_size
            .map_or(Some(probe_size), |existing_size| {
                Some(std::cmp::min(probe_size, existing_size))
            });

        self.update_probe_size();
        self.in_flight = false;
    }

    // Resets PMTUD internals such that PMTUD will be recalculated
    // on the next opportunity
    fn restart_pmtud(&mut self) {
        self.set_probe_size(self.maximum_supported_mtu);
        self.smallest_failed_probe_size = None;
        self.largest_successful_probe_size = None;
        self.pmtu = None;
    }

    // Checks that a probe of PMTU size can be ack'd by enabling
    // a probe on the next opportunity. If this probe is dropped
    // PMTUD will restart from a fresh state
    pub fn revalidate_pmtu(&mut self) {
        if let Some(pmtu) = self.pmtu {
            self.set_probe_size(pmtu);
            self.pmtu = None;
        };
    }
}

impl std::fmt::Debug for Pmtud {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "pmtu={:?} ", self.pmtu)?;
        write!(f, "probe_size={:?} ", self.probe_size)?;
        write!(f, "should_probe={:?} ", self.should_probe())?;
        Ok(())
    }
}
