#include "doctest.h"
#include "pose_convert.h"

using namespace vrdummy;

TEST_CASE("tracking result mapping") {
    CHECK(ToTrackingResult(protocol::TrackingResult::Uninitialized) == vr::TrackingResult_Uninitialized);
    CHECK(ToTrackingResult(protocol::TrackingResult::RunningOk) == vr::TrackingResult_Running_OK);
    CHECK(ToTrackingResult(protocol::TrackingResult::RunningOutOfRange) == vr::TrackingResult_Running_OutOfRange);
    CHECK(ToTrackingResult(protocol::TrackingResult::Calibrating) == vr::TrackingResult_Calibrating_InProgress);
    CHECK(ToTrackingResult(protocol::TrackingResult::FallbackRotationOnly) == vr::TrackingResult_Fallback_RotationOnly);
}

TEST_CASE("initial pose has identity transforms") {
    const vr::DriverPose_t p = MakeInitialPose();
    CHECK(p.qWorldFromDriverRotation.w == 1.0);
    CHECK(p.qDriverFromHeadRotation.w == 1.0);
    CHECK(p.qRotation.w == 1.0);
    CHECK_FALSE(p.poseIsValid);
    CHECK(p.deviceIsConnected);
}

TEST_CASE("protocol pose converts to driver pose") {
    protocol::Pose src;
    src.device_id = 1;
    src.flags.bits = protocol::PoseFlags::kPoseValid | protocol::PoseFlags::kConnected;
    src.tracking_result = protocol::TrackingResult::RunningOk;
    src.position = {0.1, 1.6, -0.25};
    src.rotation = {0.9238795, 0.0, 0.3826834, 0.0};
    src.linear_velocity = {0.5, 0.0, 0.0};
    src.angular_velocity = {0.0, 0.25, 0.0};

    const vr::DriverPose_t p = ToDriverPose(src, true);
    CHECK(p.vecPosition[0] == doctest::Approx(0.1));
    CHECK(p.vecPosition[1] == doctest::Approx(1.6));
    CHECK(p.vecPosition[2] == doctest::Approx(-0.25));
    CHECK(p.qRotation.w == doctest::Approx(0.9238795));
    CHECK(p.qRotation.y == doctest::Approx(0.3826834));
    CHECK(p.vecVelocity[0] == doctest::Approx(0.5));
    CHECK(p.vecAngularVelocity[1] == doctest::Approx(0.25));
    CHECK(p.poseIsValid);
    CHECK(p.deviceIsConnected);
    CHECK(p.result == vr::TrackingResult_Running_OK);
    CHECK(p.shouldApplyHeadModel);
    CHECK(p.poseTimeOffset == 0.0);
}

TEST_CASE("disconnected pose keeps position but loses validity") {
    protocol::Pose src;
    src.flags.bits = protocol::PoseFlags::kPoseValid | protocol::PoseFlags::kConnected;
    src.tracking_result = protocol::TrackingResult::RunningOk;
    src.position = {1.0, 2.0, 3.0};
    const vr::DriverPose_t p = MarkOutOfRange(ToDriverPose(src, false));
    CHECK(p.vecPosition[2] == doctest::Approx(3.0));
    CHECK_FALSE(p.poseIsValid);
    CHECK(p.result == vr::TrackingResult_Running_OutOfRange);
}
