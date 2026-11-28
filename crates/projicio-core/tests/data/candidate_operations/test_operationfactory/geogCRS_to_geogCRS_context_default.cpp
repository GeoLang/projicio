TEST(operation, geogCRS_to_geogCRS_context_default) {
    auto authFactory =
        AuthorityFactory::create(DatabaseContext::create(), "EPSG");
    auto ctxt = CoordinateOperationContext::create(authFactory, nullptr, 0);
    ctxt->setSpatialCriterion(
        CoordinateOperationContext::SpatialCriterion::PARTIAL_INTERSECTION);
    ctxt->setAllowUseIntermediateCRS(
        CoordinateOperationContext::IntermediateCRSUse::NEVER);

    // Directly found in database
    {
        auto list = CoordinateOperationFactory::create()->createOperations(
            authFactory->createCoordinateReferenceSystem("4179"), // Pulkovo 42
            authFactory->createCoordinateReferenceSystem("4258"), // ETRS89
            ctxt);
        ASSERT_EQ(list.size(), 3U);
        // Romania has a larger area than Poland (given our approx formula)
        EXPECT_EQ(list[0]->getEPSGCode(), 15994); // Romania - 3m
        EXPECT_EQ(list[1]->getEPSGCode(), 1644);  // Poland - 1m
        EXPECT_EQ(list[2]->nameStr(),
                  "Ballpark geographic offset from Pulkovo 1942(58) to ETRS89");

        EXPECT_EQ(
            list[0]->exportToPROJString(PROJStringFormatter::create().get()),
            "+proj=pipeline +step +proj=axisswap +order=2,1 +step "
            "+proj=unitconvert +xy_in=deg +xy_out=rad +step +proj=push +v_3 "
            "+step +proj=cart +ellps=krass +step +proj=helmert +x=2.3287 "
            "+y=-147.0425 +z=-92.0802 +rx=0.3092483 +ry=-0.32482185 "
            "+rz=-0.49729934 +s=5.68906266 +convention=coordinate_frame +step "
            "+inv +proj=cart +ellps=GRS80 +step +proj=pop +v_3 +step "
            "+proj=unitconvert +xy_in=rad +xy_out=deg +step +proj=axisswap "
            "+order=2,1");
    }

    // Reverse case
    {
        auto list = CoordinateOperationFactory::create()->createOperations(
            authFactory->createCoordinateReferenceSystem("4258"),
            authFactory->createCoordinateReferenceSystem("4179"), ctxt);
        ASSERT_EQ(list.size(), 3U);
        // Romania has a larger area than Poland (given our approx formula)
        EXPECT_EQ(list[0]->nameStr(),
                  "Inverse of Pulkovo 1942(58) to ETRS89 (4)"); // Romania - 3m

        EXPECT_EQ(
            list[0]->exportToPROJString(PROJStringFormatter::create().get()),
            "+proj=pipeline +step +proj=axisswap +order=2,1 +step "
            "+proj=unitconvert +xy_in=deg +xy_out=rad +step +proj=push +v_3 "
            "+step +proj=cart +ellps=GRS80 +step +inv +proj=helmert +x=2.3287 "
            "+y=-147.0425 +z=-92.0802 +rx=0.3092483 +ry=-0.32482185 "
            "+rz=-0.49729934 +s=5.68906266 +convention=coordinate_frame +step "
            "+inv +proj=cart +ellps=krass +step +proj=pop +v_3 +step "
            "+proj=unitconvert +xy_in=rad +xy_out=deg +step +proj=axisswap "
            "+order=2,1");
    }
}

