// Conversion from protocol poses to OpenVR driver poses.
// Depends only on openvr_driver.h types, not on the runtime context.
#pragma once

#include "openvr_driver.h"
#include "protocol.h"

namespace vrdummy {

inline vr::ETrackingResult ToTrackingResult(protocol::TrackingResult r) {
    switch (r) {
        case protocol::TrackingResult::Uninitialized: return vr::TrackingResult_Uninitialized;
        case protocol::TrackingResult::RunningOk: return vr::TrackingResult_Running_OK;
        case protocol::TrackingResult::RunningOutOfRange: return vr::TrackingResult_Running_OutOfRange;
        case protocol::TrackingResult::Calibrating: return vr::TrackingResult_Calibrating_InProgress;
        case protocol::TrackingResult::FallbackRotationOnly: return vr::TrackingResult_Fallback_RotationOnly;
    }
    return vr::TrackingResult_Uninitialized;
}

// A pose with identity driver and head transforms, not yet valid.
inline vr::DriverPose_t MakeInitialPose() {
    vr::DriverPose_t pose{};
    pose.qWorldFromDriverRotation.w = 1.0;
    pose.qDriverFromHeadRotation.w = 1.0;
    pose.qRotation.w = 1.0;
    pose.poseIsValid = false;
    pose.deviceIsConnected = true;
    pose.result = vr::TrackingResult_Uninitialized;
    return pose;
}

inline vr::DriverPose_t ToDriverPose(const protocol::Pose& p, bool should_apply_head_model) {
    vr::DriverPose_t pose = MakeInitialPose();
    pose.poseTimeOffset = 0.0;
    pose.qRotation.w = p.rotation[0];
    pose.qRotation.x = p.rotation[1];
    pose.qRotation.y = p.rotation[2];
    pose.qRotation.z = p.rotation[3];
    for (int i = 0; i < 3; ++i) {
        pose.vecPosition[i] = p.position[i];
        pose.vecVelocity[i] = p.linear_velocity[i];
        pose.vecAngularVelocity[i] = p.angular_velocity[i];
    }
    pose.poseIsValid = p.flags.pose_valid();
    pose.deviceIsConnected = p.flags.connected();
    pose.result = ToTrackingResult(p.tracking_result);
    pose.shouldApplyHeadModel = should_apply_head_model;
    return pose;
}

// Marks a pose as tracking lost while keeping its last position.
inline vr::DriverPose_t MarkOutOfRange(vr::DriverPose_t pose) {
    pose.result = vr::TrackingResult_Running_OutOfRange;
    pose.poseIsValid = false;
    return pose;
}

}  // namespace vrdummy
